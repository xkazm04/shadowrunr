// `pnpm -C packages/contract check`: the drift gate. Re-exports the schema from Rust and
// regenerates into a temp directory, then diffs against the committed schema/ and
// src/generated/. Exits 1 on any difference, so a Rust change forces regeneration.

import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { GENERATED_FILES, SCHEMA_FILE, generate } from "./codegen.mjs";

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
  const pairs = [
    [join("schema", SCHEMA_FILE), join(tmp, "schema", SCHEMA_FILE)],
    ...GENERATED_FILES.map((f) => [join("src", "generated", f), join(tmp, "generated", f)]),
  ];
  const stale = [];
  for (const [committed, fresh] of pairs) {
    if ((await read(join(pkg, committed))) !== (await read(fresh))) stale.push(committed);
  }
  if (stale.length > 0) {
    console.error(`contract drift: ${stale.join(", ")} differ from the Rust types.`);
    console.error("Run `pnpm -C packages/contract generate` and commit the result.");
    process.exitCode = 1;
  } else {
    console.log(`contract in sync (${pairs.length} files)`);
  }
} finally {
  await rm(tmp, { recursive: true, force: true });
}
