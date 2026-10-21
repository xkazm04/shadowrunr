// `pnpm -C packages/contract check`: the drift gate. Re-exports the schema from Rust and
// regenerates into a temp directory, then diffs against the committed schema/ and the WHOLE
// src/generated/ tree: a changed, missing or stray file (e.g. a results/<Def>.js left behind
// after its def was removed) is drift. Exits 1 naming each file, so a Rust change forces
// regeneration.

import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { SCHEMA_FILE, diffTrees, generate, listTree } from "./codegen.mjs";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..");
const tmp = await mkdtemp(join(tmpdir(), "shadowrunr-contract-"));

const read = (p) =>
  readFile(p, "utf8").then(
    (t) => t.replace(/\r\n/g, "\n"),
    (e) => `missing: ${e.code}`,
  );

try {
  const args = ["run", "-q", "-p", "shadowrunr-types", "--example", "export-schema"];
  execFileSync("cargo", [...args, "--", "--out", join(tmp, "schema")], {
    cwd: pkg,
    stdio: ["ignore", "ignore", "inherit"],
  });
  await generate(join(tmp, "schema"), join(tmp, "generated"));
  const schemaFile = join("schema", SCHEMA_FILE);
  const stale = [];
  if ((await read(join(pkg, schemaFile))) !== (await read(join(tmp, schemaFile)))) stale.push(schemaFile);
  const gen = join("src", "generated");
  const drift = await diffTrees(join(pkg, gen), join(tmp, "generated"));
  const at = (f) => `src/generated/${f}`;
  stale.push(
    ...drift.changed.map((f) => `${at(f)} (changed)`),
    ...drift.missing.map((f) => `${at(f)} (missing)`),
    ...drift.extra.map((f) => `${at(f)} (not generated)`),
  );
  if (stale.length > 0) {
    console.error(`contract drift: ${stale.join(", ")} differ from the Rust types.`);
    console.error("Run `pnpm -C packages/contract generate` and commit the result.");
    process.exitCode = 1;
  } else {
    const files = await listTree(join(tmp, "generated"));
    console.log(`contract in sync (schema + ${files.length} generated files)`);
  }
} finally {
  await rm(tmp, { recursive: true, force: true });
}
