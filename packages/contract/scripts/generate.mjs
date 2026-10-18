// `pnpm -C packages/contract generate`: export the schema from Rust, then regenerate
// src/generated/. Pass --no-export to regenerate from the committed schema only.

import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { generate } from "./codegen.mjs";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..");
const schemaDir = join(pkg, "schema");

if (!process.argv.includes("--no-export")) {
  const args = ["run", "-q", "-p", "shadowrunr-types", "--example", "export-schema"];
  execFileSync("cargo", [...args, "--", "--out", schemaDir], { cwd: pkg, stdio: "inherit" });
}
const written = await generate(schemaDir, join(pkg, "src", "generated"));
console.log(`generated ${written.join(", ")}`);
