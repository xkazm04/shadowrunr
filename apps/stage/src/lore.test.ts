// The stage keeps copies of display data owned by other packages; these tests fail on drift.

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { RANKS } from "./contract";
import { FOILS, RANK_TITLE, RANK_WEIGHT, foilInfo, isAlleged } from "./lore";

const repo = join(__dirname, "..", "..", "..");

describe("copies stay in sync with their sources", () => {
  it("RANK_WEIGHT matches crates/shadowrunr-segments/data/ranks.toml", () => {
    const toml = readFileSync(join(repo, "crates", "shadowrunr-segments", "data", "ranks.toml"), "utf8");
    const pairs = [...toml.matchAll(/rank\s*=\s*"([a-z_]+)"\s*\nweight\s*=\s*([\d.]+)/g)].map((m) => [m[1], Number(m[2])]);
    expect(Object.fromEntries(pairs)).toEqual(RANK_WEIGHT);
  });

  it("RANK_TITLE matches characters/malakor/character.toml [ranks]", () => {
    const toml = readFileSync(join(repo, "characters", "malakor", "character.toml"), "utf8");
    const block = toml.split("[ranks]")[1] ?? "";
    for (const r of RANKS) {
      const m = new RegExp(`^${r}\\s*=\\s*"([^"]+)"`, "m").exec(block);
      expect(m?.[1], r).toBe(RANK_TITLE[r]);
    }
  });

  it("FOILS covers every foil file in characters/malakor/foils", () => {
    const files = readdirSync(join(repo, "characters", "malakor", "foils")).filter((f) => f.endsWith(".md"));
    expect(Object.keys(FOILS).sort()).toEqual(files.map((f) => f.replace(/\.md$/, "")).sort());
  });

  it("fixtures/demo-case.json is a byte copy of the segments demo case", () => {
    const a = readFileSync(join(repo, "crates", "shadowrunr-segments", "data", "demo-case.json"), "utf8");
    const b = readFileSync(join(__dirname, "..", "fixtures", "demo-case.json"), "utf8");
    expect(b.replace(/\r\n/g, "\n")).toBe(a.replace(/\r\n/g, "\n"));
  });
});

describe("lore helpers", () => {
  it("stamps everything below DOC as alleged", () => {
    expect(isAlleged("DOC")).toBe(false);
    for (const g of ["REP", "CLM", "SPC"] as const) expect(isAlleged(g)).toBe(true);
  });

  it("title-cases an unknown foil id", () => {
    expect(foilInfo("the-night-auditor").name).toBe("The Night Auditor");
    expect(foilInfo("the-inspector").name).toBe("The Inspector");
  });
});
