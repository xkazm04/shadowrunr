#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// ci-lanes: the CI lane set is derived from the workspace, not remembered by hand.
//
// Expected lanes:
//   - every workspace package (the `packages:` globs in pnpm-workspace.yaml, `dir/*` form) x each
//     gate script it declares, out of {check, typecheck, test, lint, build};
//   - every Python suite under tools/*: a `tests/` folder holding test_*.py -> `unittest discover`,
//     otherwise top-level test_*.py -> `unittest <those modules>`.
// Rust lanes are NOT derived: the `rust` job is fixed commands (fmt, clippy, test) for the root
// cargo workspace plus the same three for apps/console/src-tauri (its own workspace, via
// --manifest-path), so `cargo` run lines are ignored here.
//
// Scheduled lanes are read from the workflow's `run:` lines (with each step's
// `working-directory:`). Any line that invokes pnpm or python in a form this planner does not know
// is reported as unparseable - never skipped - and a lane naming a package, script or suite that
// does not exist is stale. `--check` exits 1 on any uncovered, stale or unparseable lane and always
// prints the denominator it checked.
//
// Usage: node scripts/ci-lanes.mjs --check [--root <dir>] [--workflow <file>]

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const GATE_SCRIPTS = ["check", "typecheck", "test", "lint", "build"];

const posix = (p) => p.split("\\").join("/");
const isDir = (p) => existsSync(p) && statSync(p).isDirectory();
const unquote = (s) => s.trim().replace(/^["']|["']$/g, "");
const testModules = (dir) =>
  readdirSync(dir)
    .filter((f) => /^test_\w+\.py$/.test(f))
    .map((f) => f.slice(0, -3))
    .sort();

/** The `packages:` globs of pnpm-workspace.yaml. Only `dir/*` and plain `dir` forms are known. */
export function workspaceGlobs(text) {
  const globs = [];
  let inPackages = false;
  for (const raw of text.split(/\r?\n/)) {
    if (/^packages:\s*$/.test(raw)) {
      inPackages = true;
      continue;
    }
    if (!inPackages) continue;
    if (/^\S/.test(raw)) break;
    const item = raw.match(/^\s+-\s+(.+?)\s*$/);
    if (item) globs.push(unquote(item[1]));
  }
  return globs;
}

/** Workspace package dirs (repo-relative, posix) for the given root. */
export function workspacePackages(root) {
  const file = join(root, "pnpm-workspace.yaml");
  const globs = workspaceGlobs(readFileSync(file, "utf8"));
  const dirs = [];
  for (const glob of globs) {
    if (glob.endsWith("/*") && !glob.slice(0, -2).includes("*")) {
      const base = glob.slice(0, -2);
      if (!isDir(join(root, base))) continue;
      for (const name of readdirSync(join(root, base)).sort()) {
        if (existsSync(join(root, base, name, "package.json"))) dirs.push(`${base}/${name}`);
      }
    } else if (!glob.includes("*")) {
      if (existsSync(join(root, glob, "package.json"))) dirs.push(posix(glob));
    } else {
      throw new Error(`pnpm-workspace.yaml: glob \`${glob}\` is a form ci-lanes cannot replay`);
    }
  }
  return dirs;
}

function scriptsOf(root, dir) {
  const file = join(root, dir, "package.json");
  if (!existsSync(file)) return null;
  return JSON.parse(readFileSync(file, "utf8")).scripts ?? {};
}

/** Python suites under tools/*: {dir, modules|null} where null means `unittest discover`. */
export function pythonSuites(root) {
  const tools = join(root, "tools");
  if (!isDir(tools)) return [];
  const suites = [];
  for (const name of readdirSync(tools).sort()) {
    const dir = join(tools, name);
    if (!isDir(dir) || name.startsWith(".") || name === "__pycache__") continue;
    if (isDir(join(dir, "tests")) && testModules(join(dir, "tests")).length > 0) {
      suites.push({ dir: `tools/${name}`, modules: null });
    } else if (testModules(dir).length > 0) {
      suites.push({ dir: `tools/${name}`, modules: testModules(dir) });
    }
  }
  return suites;
}

const pyId = (dir, modules) =>
  modules === null ? `${dir} unittest discover` : `${dir} unittest ${modules.join(" ")}`;

/** Every lane the workspace declares, as stable ids. */
export function deriveLanes(root) {
  const lanes = [];
  for (const dir of workspacePackages(root)) {
    const scripts = scriptsOf(root, dir);
    for (const script of GATE_SCRIPTS) if (script in scripts) lanes.push(`${dir} ${script}`);
  }
  for (const suite of pythonSuites(root)) lanes.push(pyId(suite.dir, suite.modules));
  return lanes;
}

/**
 * The run commands of a GitHub workflow, each with its step's working-directory. A line scanner
 * over the `steps:` lists: enough for the block style this repo writes, and anything it misreads
 * surfaces as an unparseable or uncovered lane rather than a silent pass.
 */
export function workflowCommands(text) {
  const lines = text.split(/\r?\n/);
  const commands = [];
  let stepsIndent = -1;
  let itemIndent = -1;
  let step = null;
  const flush = () => {
    if (step) for (const cmd of step.runs) commands.push({ cmd, wd: step.wd });
    step = null;
  };
  const indentOf = (s) => s.match(/^ */)[0].length;
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    if (/^\s*(#.*)?$/.test(raw)) continue;
    const indent = indentOf(raw);
    if (/^\s*steps:\s*$/.test(raw)) {
      flush();
      stepsIndent = indent;
      itemIndent = -1;
      continue;
    }
    if (stepsIndent < 0) continue;
    if (indent <= stepsIndent && !(indent === stepsIndent && raw.trimStart().startsWith("- "))) {
      flush();
      stepsIndent = -1;
      continue;
    }
    const isItem = raw.trimStart().startsWith("- ");
    if (isItem && (itemIndent < 0 || indent === itemIndent)) {
      flush();
      itemIndent = indent;
      step = { runs: [], wd: "." };
    }
    if (!step) continue;
    const body = isItem && indent === itemIndent ? raw.slice(indent + 2) : raw.trimStart();
    const keyIndent = isItem && indent === itemIndent ? indent + 2 : indent;
    const kv = body.match(/^([\w-]+):\s*(.*?)\s*$/);
    if (!kv) continue;
    const [, key, value] = kv;
    if (key === "working-directory") step.wd = posix(unquote(value)).replace(/^\.\/|\/$/g, "") || ".";
    if (key !== "run") continue;
    if (/^[|>][-+]?$/.test(value)) {
      while (i + 1 < lines.length) {
        const next = lines[i + 1];
        if (!/^\s*$/.test(next) && indentOf(next) <= keyIndent) break;
        i++;
        const cmd = next.trim();
        if (cmd && !cmd.startsWith("#")) step.runs.push(cmd);
      }
    } else if (value) {
      step.runs.push(unquote(value));
    }
  }
  flush();
  return commands;
}

const INVOKES = /(^|[\s;&|(`])(pnpm|python3?|py|pip3?)(?=\s|$)/;

/**
 * One command -> a lane it schedules, a setup command, an ignored (non pnpm/python) command, or
 * unparseable.
 */
export function classify(cmd, wd) {
  if (!INVOKES.test(cmd)) return { kind: "ignored" };
  const words = cmd.split(/\s+/);
  if (/^pnpm install( --frozen-lockfile)?$/.test(cmd)) return { kind: "setup" };
  if (/^python3? -m pip install( [\w.=<>-]+)+$/.test(cmd)) return { kind: "setup" };
  let m = cmd.match(/^pnpm -C (\S+) (?:run )?([\w:-]+)$/);
  if (m) {
    const dir = posix(m[1]).replace(/^\.\/|\/$/g, "");
    return { kind: "pnpm", dir, script: m[2], id: `${dir} ${m[2]}` };
  }
  m = cmd.match(/^pnpm (?:run )?([\w:-]+)$/);
  if (m && wd !== "." && m[1] !== "install") return { kind: "pnpm", dir: wd, script: m[1], id: `${wd} ${m[1]}` };
  if (words[0] === "python" || words[0] === "python3") {
    if (words[1] === "-m" && words[2] === "unittest") {
      const rest = words.slice(3);
      const flags = rest.join(" ");
      if (flags === "discover -t . -s tests" || flags === "discover -s tests -t .") {
        return { kind: "python", dir: wd, modules: null, id: pyId(wd, null) };
      }
      if (rest.length > 0 && rest.every((w) => /^test_\w+$/.test(w))) {
        const modules = [...rest].sort();
        return { kind: "python", dir: wd, modules, id: pyId(wd, modules) };
      }
    }
  }
  return { kind: "unparseable" };
}

/** The whole plan: derived lanes against what the workflow schedules. */
export function plan(root, workflowText) {
  const derived = deriveLanes(root);
  const derivedSet = new Set(derived);
  const packages = new Set(workspacePackages(root));
  const scheduled = new Set();
  const stale = [];
  const unparseable = [];
  for (const { cmd, wd } of workflowCommands(workflowText)) {
    const c = classify(cmd, wd);
    if (c.kind === "ignored" || c.kind === "setup") continue;
    if (c.kind === "unparseable") {
      unparseable.push(wd === "." ? cmd : `${cmd} (in ${wd})`);
      continue;
    }
    if (derivedSet.has(c.id)) {
      scheduled.add(c.id);
      continue;
    }
    // Scheduled but not derived: a package, script or suite that does not exist (stale), or a
    // script the package declares outside the gate set, which is a lane all the same.
    if (c.kind === "pnpm" && packages.has(c.dir)) {
      const scripts = scriptsOf(root, c.dir);
      if (c.script in scripts) continue;
    }
    stale.push(c.id);
  }
  const covered = derived.filter((id) => scheduled.has(id));
  const uncovered = derived.filter((id) => !scheduled.has(id));
  return { derived, covered, uncovered, stale, unparseable };
}

export function report(result) {
  const out = [];
  for (const id of result.uncovered) out.push(`uncovered lane: ${id}`);
  for (const id of result.stale) out.push(`stale lane: ${id} (no such workspace package, script or suite)`);
  for (const cmd of result.unparseable) out.push(`unparseable lane: ${cmd}`);
  out.push(
    `${result.derived.length} lanes derived, ${result.covered.length} covered, ` +
      `${result.uncovered.length} uncovered, ${result.stale.length} stale, ` +
      `${result.unparseable.length} unparseable`,
  );
  return out;
}

export const failed = (r) => r.uncovered.length + r.stale.length + r.unparseable.length > 0;

function main(argv) {
  const arg = (name) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const here = dirname(fileURLToPath(import.meta.url));
  const root = resolve(arg("--root") ?? join(here, ".."));
  const workflow = resolve(arg("--workflow") ?? join(root, ".github", "workflows", "ci.yml"));
  const result = plan(root, readFileSync(workflow, "utf8"));
  const lines = report(result);
  if (argv.includes("--check") && failed(result)) {
    for (const line of lines) console.error(line);
    return 1;
  }
  if (!argv.includes("--check")) for (const id of result.derived) console.log(`lane: ${id}`);
  for (const line of lines) console.log(line);
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
