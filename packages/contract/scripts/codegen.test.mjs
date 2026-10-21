// The generator's own contract: one validator module per result def, a small eager module for
// the socket, byte-identical output, and a drift diff that sees the whole generated tree.
import { cp, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import * as codegen from "./codegen.mjs";
import { SCHEMA_FILE, diffTrees, generate } from "./codegen.mjs";

const pkg = join(dirname(fileURLToPath(import.meta.url)), "..");
const schemaDir = join(pkg, "schema");
const committed = join(pkg, "src", "generated");

let tmp;
let first;
let second;
let defs;

beforeAll(async () => {
  tmp = await mkdtemp(join(tmpdir(), "shadowrunr-codegen-test-"));
  first = join(tmp, "a");
  second = join(tmp, "b");
  await generate(schemaDir, first);
  await generate(schemaDir, second);
  const schema = JSON.parse(await readFile(join(schemaDir, SCHEMA_FILE), "utf8"));
  defs = [...new Set(Object.values(schema.properties).map((m) => m.properties.result.$ref.slice("#/$defs/".length)))].sort();
}, 180_000);

afterAll(async () => {
  if (tmp) await rm(tmp, { recursive: true, force: true });
});

describe("generate", () => {
  it("writes exactly one validator module (js + d.ts) per distinct method result def", async () => {
    const files = (await readdir(join(first, "results"))).sort();
    expect(defs.length).toBeGreaterThan(30);
    expect(files).toEqual(defs.flatMap((d) => [`${d}.d.ts`, `${d}.js`]).sort());
  });

  it("keeps only ServerFrame and ErrorEnvelope in the eager validators module", async () => {
    const js = await readFile(join(first, "validators.js"), "utf8");
    const exported = [...js.matchAll(/^export const (\w+)/gm)].map((m) => m[1]).sort();
    expect(exported).toEqual(["validateErrorEnvelope", "validateServerFrame"]);
    const dts = await readFile(join(first, "validators.d.ts"), "utf8");
    expect([...dts.matchAll(/export declare const (\w+)/g)].map((m) => m[1]).sort()).toEqual(exported);
  });

  it("is byte-identical when run twice", async () => {
    expect(await diffTrees(first, second)).toEqual({ missing: [], extra: [], changed: [] });
  });

  it("matches the committed src/generated tree (regenerate after a schema change)", async () => {
    expect(await diffTrees(committed, first)).toEqual({ missing: [], extra: [], changed: [] });
  });

  it("compiles a result module from its own def's closure only", async () => {
    const js = await readFile(join(first, "results", "RunsPage.js"), "utf8");
    expect(js).toMatch(/^export const validate = /m);
    expect(js).toMatch(/\$defs\/RunRow\b/); // its own closure is there
    expect(js).not.toMatch(/\$defs\/(ClipCandidate|ReviewSession|ReconcileReport|ServerFrame)\b/);
  });

  it("loads each method's module lazily from meta.ts, with no static validator import", async () => {
    const meta = await readFile(join(first, "meta.ts"), "utf8");
    expect(meta).not.toMatch(/^import \* as/m);
    expect(meta).not.toMatch(/^import (?!type )/m);
    expect(meta).toContain(`"runs.page": () => import("./results/RunsPage.js").then((m) => m.validate),`);
  });
});

describe("diffTrees (the drift gate's comparison)", () => {
  it("names a deleted generated file as missing and a stray one as extra", async () => {
    const copy = join(tmp, "c");
    await cp(first, copy, { recursive: true });
    await rm(join(copy, "results", "RunsPage.js"));
    await writeFile(join(copy, "results", "Stray.js"), "export const validate = () => true;\n");
    await writeFile(join(copy, "meta.ts"), "// edited\n");
    expect(await diffTrees(copy, first)).toEqual({
      missing: ["results/RunsPage.js"],
      extra: ["results/Stray.js"],
      changed: ["meta.ts"],
    });
  });

  it("ignores CRLF vs LF (a Windows checkout is not drift)", async () => {
    const copy = join(tmp, "d");
    await cp(first, copy, { recursive: true });
    const p = join(copy, "results", "RunsPage.d.ts");
    await writeFile(p, (await readFile(p, "utf8")).replace(/\n/g, "\r\n"));
    expect(await diffTrees(copy, first)).toEqual({ missing: [], extra: [], changed: [] });
  });
});

describe("forAjv (tagged unions validate by their tag)", () => {
  it("adds a discriminator to exactly the oneOfs whose every alternative requires one common const property", async () => {
    expect(typeof codegen.forAjv).toBe("function");
    const schema = JSON.parse(await readFile(join(schemaDir, SCHEMA_FILE), "utf8"));
    const prepared = codegen.forAjv(schema);
    const tagged = Object.entries(prepared.$defs)
      .filter(([, d]) => d.discriminator)
      .map(([n, d]) => `${n}(${d.discriminator.propertyName})`)
      .sort();
    expect(tagged).toEqual(["CalibrationOrigin(kind)", "Command(cmd)", "EngineError(kind)", "Op(op)", "StreamEvent(type)", "VoiceFrame(type)"]);
    // Derived from the schema, not a hand list: a new tagged union is picked up, an untagged one is not.
    const tag = (v) => ({ type: "object", properties: { t: { const: v, type: "string" }, x: { type: "integer" } }, required: ["t"] });
    const probe = { ...schema, $defs: { ...schema.$defs, NewUnion: { oneOf: [tag("a"), tag("b")] }, Loose: { oneOf: [tag("a"), { type: "object", properties: {} }] } } };
    const out = codegen.forAjv(probe);
    expect(out.$defs.NewUnion.discriminator).toEqual({ propertyName: "t" });
    expect(out.$defs.Loose.discriminator).toBeUndefined();
    // The committed schema object is not mutated.
    expect(schema.$defs.StreamEvent.discriminator).toBeUndefined();
  });

  it("compiles the tagged validators with ajv discriminator dispatch", async () => {
    const js = await readFile(join(first, "validators.js"), "utf8");
    expect(js).toMatch(/discriminator/);
    expect([...js.matchAll(/^export const (\w+)/gm)].map((m) => m[1]).sort()).toEqual([
      "validateErrorEnvelope",
      "validateEventFrame",
      "validateServerFrame",
      "validateStreamEvent",
    ]);
  });
});
