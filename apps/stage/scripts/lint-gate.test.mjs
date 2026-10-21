// The stage lint gate, proven red on seeded violations (node --test; run by `pnpm lint`).
// Seeds go to ESLint on stdin under a real file name, so no source file is touched. ESLint does
// not apply suppressions to piped code, so the ratchet cases lint a seeded copy of App.tsx written
// beside it for the test's length (src/__lint_gate__.tsx, removed after) against a temporary copy
// of eslint-suppressions.json whose App.tsx entry is moved to that name.
// STAGE_LINT_CONFIG=<path> lints with another config (how the gate was watched red before it
// existed: the root config at the parent commit, which ignored apps/stage).

import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";

const STAGE = join(dirname(fileURLToPath(import.meta.url)), "..");
const ROOT = join(STAGE, "..", "..");
const ESLINT = join(ROOT, "node_modules", "eslint", "bin", "eslint.js");
const CONFIG = process.env.STAGE_LINT_CONFIG ? ["--config", process.env.STAGE_LINT_CONFIG] : [];
const SUPPRESSIONS = join(STAGE, "eslint-suppressions.json");

const tmp = mkdtempSync(join(tmpdir(), "stage-lint-gate-"));
const SEED_FILE = "src/__lint_gate__.tsx";
after(() => {
  rmSync(tmp, { recursive: true, force: true });
  rmSync(join(STAGE, SEED_FILE), { force: true });
});

/** Lint `text` as if it were the stage file `file` (relative to apps/stage). */
function lint(file, text, extra = []) {
  const r = spawnSync(process.execPath, [ESLINT, ...CONFIG, "--max-warnings", "0", "--stdin", "--stdin-filename", file, ...extra], {
    cwd: STAGE,
    input: text,
    encoding: "utf8",
  });
  return { status: r.status, out: `${r.stdout}${r.stderr}` };
}

const read = (file) => readFileSync(join(STAGE, file), "utf8");

/** A suppressions copy in which App.tsx's baseline belongs to the seed file; returns its path. */
function baselineCopy() {
  const all = JSON.parse(readFileSync(SUPPRESSIONS, "utf8"));
  const copy = join(tmp, "suppressions.json");
  writeFileSync(copy, JSON.stringify({ [SEED_FILE]: all["src/App.tsx"] }, null, 2));
  return copy;
}

/** Lint `text` written as the seed file, against the suppressions file `copy`. */
function lintSeed(text, copy, extra = []) {
  writeFileSync(join(STAGE, SEED_FILE), text);
  const r = spawnSync(process.execPath, [ESLINT, ...CONFIG, "--max-warnings", "0", "--suppressions-location", copy, ...extra, SEED_FILE], {
    cwd: STAGE,
    encoding: "utf8",
  });
  return { status: r.status, out: `${r.stdout}${r.stderr}` };
}
/** Append a seed after the file's last import, so it parses as module code. */
const seeded = (file, seed) => `${read(file)}\n${seed}\n`;

test("case 1: the stage has a lint script and its tree lints clean with --max-warnings 0", () => {
  const pkg = JSON.parse(read("package.json"));
  assert.match(pkg.scripts?.lint ?? "", /^eslint src --max-warnings 0\b/);
  const r = spawnSync(process.execPath, [ESLINT, ...CONFIG, "src", "--max-warnings", "0"], { cwd: STAGE, encoding: "utf8" });
  assert.equal(r.status, 0, `${r.stdout}${r.stderr}`);
  // and it read the files: a seeded file is linted, not ignored
  assert.doesNotMatch(lint("src/source.ts", read("src/source.ts")).out, /File ignored/);
});

test("case 2: a seeded `as unknown as` in source.ts is red", () => {
  const r = lint("src/source.ts", seeded("src/source.ts", "export const seedCast = (y: string) => y as unknown as number;"));
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /No `as unknown as`/);
});

test("case 3: JSON.parse is red in OnAir.tsx, green in the two declared decoders", () => {
  const seed = 'export const seedParse = (s: string): unknown => JSON.parse(s);';
  const red = lint("src/OnAir.tsx", seeded("src/OnAir.tsx", seed));
  assert.equal(red.status, 1, red.out);
  assert.match(red.out, /JSON\.parse lives only/);
  for (const door of ["src/clip.ts", "src/voice/frame.ts"]) {
    assert.match(read(door), /JSON\.parse\(/, `${door} still holds its parse`);
    const r = lint(door, seeded(door, seed));
    assert.equal(r.status, 0, `${door}: ${r.out}`);
  }
});

test("case 4: a raw keydown listener is red in App.tsx, green in Controls.tsx", () => {
  const seed = 'export function seedKeys(f: () => void) { window.addEventListener("keydown", f); }';
  const red = lint("src/App.tsx", seeded("src/App.tsx", seed));
  assert.equal(red.status, 1, red.out);
  assert.match(red.out, /No raw keyboard listeners/);
  const ok = lint("src/Controls.tsx", seeded("src/Controls.tsx", seed));
  assert.equal(ok.status, 0, ok.out);
});

test("case 5: one react-hooks/refs violation beyond the baseline in App.tsx is red", () => {
  const app = read("src/App.tsx");
  const copy = baselineCopy();
  assert.equal(lintSeed(app, copy).status, 0, "App.tsx as it is sits within its baseline");
  // the seed: App's own render-time ref write, written once more
  const line = "  speedRef.current = speed;\n";
  const lf = app.replace(/\r\n/g, "\n");
  assert.ok(lf.includes(line), "the seed's model line is still in App.tsx");
  const seed = lf.replace(line, line + line);
  const r = lintSeed(seed, copy);
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /react-hooks\/refs/);
});

test("case 6: fixing a baselined violation and pruning lowers the count, and lint stays green", () => {
  const copy = baselineCopy();
  const count = () => JSON.parse(readFileSync(copy, "utf8"))[SEED_FILE]["react-hooks/refs"].count;
  const before = count();
  const line = "  speedRef.current = speed;\n";
  const app = read("src/App.tsx").replace(/\r\n/g, "\n");
  assert.ok(app.includes(line), "the seed's baselined line is still in App.tsx");
  const fixed = app.replace(line, "");
  const pruned = lintSeed(fixed, copy, ["--prune-suppressions"]);
  assert.equal(pruned.status, 0, pruned.out);
  assert.equal(count(), before - 1);
  const again = lintSeed(fixed, copy);
  assert.equal(again.status, 0, again.out);
  // the ratchet only moves down: the unfixed file no longer fits the pruned baseline
  assert.equal(lintSeed(app, copy).status, 1);
});

test("case 7: eslint.config.js has no ignore entry for apps/stage", () => {
  const config = readFileSync(process.env.STAGE_LINT_CONFIG ?? join(ROOT, "eslint.config.js"), "utf8");
  const ignores = config.slice(config.indexOf("ignores:"), config.indexOf("]", config.indexOf("ignores:")));
  assert.doesNotMatch(ignores, /apps\/stage/);
});
