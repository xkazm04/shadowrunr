// The stage keeps copies of display data owned by other packages; these tests fail on drift.

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { GRADE_COLORS, RANK_COLORS } from "@shadowrunr/tokens";
import { describe, expect, it } from "vitest";

import { RANKS } from "./vocab";
import { DISCLOSURE, FOILS, GRADE, RANK_COLOR, RANK_TITLE, RANK_WEIGHT, disclosureText, foilInfo, isAlleged } from "./lore";

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
    // A foil is `<id>.md`; a companion such as `<id>.prompts.md` (portrait prompts) is not one.
    const files = readdirSync(join(repo, "characters", "malakor", "foils")).filter((f) => /^[a-z0-9-]+\.md$/.test(f));
    expect(Object.keys(FOILS).sort()).toEqual(files.map((f) => f.replace(/\.md$/, "")).sort());
  });
});

describe("rank and grade colours follow @shadowrunr/tokens", () => {
  it("RANK_COLOR is the token source, pinned to the Poster Wall palette", () => {
    expect(RANK_COLOR).toBe(RANK_COLORS);
    expect(RANK_COLOR).toEqual({
      thrall: "#8f877a",
      acolyte: "#cdbf9f",
      cultist: "#8fc4b0",
      warlock: "#3f9c86",
      lich: "#e8913f",
      dread_council: "#ff5a2a",
    });
  });

  it("GRADE colours are the token source, pinned to the Poster Wall palette", () => {
    for (const g of ["DOC", "REP", "CLM", "SPC"] as const) expect(GRADE[g].color, g).toBe(GRADE_COLORS[g]);
    expect(Object.fromEntries(Object.values(GRADE).map((g) => [g.code, g.color]))).toEqual({
      DOC: "#86d3bb",
      REP: "#cdbf9f",
      CLM: "#e8913f",
      SPC: "#ff5a2a",
    });
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

describe("disclosure", () => {
  it("reads text only unless a voice is configured", () => {
    expect(DISCLOSURE).toBe("AI character · text only");
    expect(disclosureText(false)).toBe("AI character · text only");
    expect(disclosureText(true)).toBe("AI character · synthetic voice");
  });
});
