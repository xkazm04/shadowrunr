// @vitest-environment node
// The owner-exception overlay (fidelity/exceptions.mjs): a sanctioned difference between the port
// and the captured winner is a named, dated entry applied at check time, never a hand-edit of the
// captured contract, and an entry that no longer matches what was captured is reported, not
// honoured and not dropped. Runs in the console suite (`pnpm -C apps/console test`, which includes
// fidelity/*.test.mjs) and alone as `pnpm -C apps/console fidelity:exceptions`.
//
// Every case here is offline except the last, which drives the real fidelity gate (Playwright and
// the contest data) and is skipped unless SHADOWRUNR_CONTEST_DATA is set AND FIDELITY_E2E=1.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { applyExceptions, describeApplied, parseExceptions } from "./exceptions.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const cli = join(here, "exceptions.mjs");

const base = {
  target: "texture",
  key: "characters.halftoneA",
  prop: "opacity",
  winner: "0.55",
  port: "0",
  why: "halftone dots under layer-1 body text hurt readability",
  decided: "2026-10-20",
  ref: "packages/tokens/REJECTED.md",
};
const contract = (opacity = "0.55") => ({ characters: { halftoneA: { opacity, mixBlendMode: "normal" } } });

const dirs = [];
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** Runs the offline check over a scratch fidelity folder holding one texture contract. */
function runCli(texture, exceptions) {
  const dir = mkdtempSync(join(tmpdir(), "sr-exceptions-"));
  dirs.push(dir);
  writeFileSync(join(dir, "contract.texture.json"), JSON.stringify(texture, null, 2) + "\n");
  writeFileSync(join(dir, "exceptions.json"), JSON.stringify({ exceptions }, null, 2) + "\n");
  return spawnSync(process.execPath, [cli, "--dir", dir], { encoding: "utf8" });
}

describe("applyExceptions", () => {
  it("case 1: an exception whose winner matches puts the port value in a copy, with no problems", () => {
    const { exceptions, problems: parsed } = parseExceptions({ exceptions: [base] });
    expect(parsed).toEqual([]);
    const original = contract();
    const r = applyExceptions(original, exceptions, "texture");
    expect(r.problems).toEqual([]);
    expect(r.contract.characters.halftoneA.opacity).toBe("0");
    expect(r.contract.characters.halftoneA.mixBlendMode).toBe("normal");
    expect(r.applied).toHaveLength(1);
    expect(original.characters.halftoneA.opacity).toBe("0.55"); // the captured contract is never edited
  });

  it("case 2: a fresh capture that moved the winner is a re-decide problem and the run exits 1", () => {
    const { exceptions } = parseExceptions({ exceptions: [base] });
    const r = applyExceptions(contract("0.6"), exceptions, "texture");
    expect(r.problems).toEqual([
      "characters.halftoneA.opacity: the winner moved from 0.55 to 0.6 under an owner exception; re-decide it",
    ]);
    expect(r.contract.characters.halftoneA.opacity).toBe("0.6");
    expect(r.applied).toEqual([]);
    expect(runCli(contract("0.6"), [base]).status).toBe(1);
  });

  it("case 3: an exception on a layer the contract does not have is reported, never dropped", () => {
    const ghost = { ...base, key: "characters.halftoneC" };
    const { exceptions } = parseExceptions({ exceptions: [ghost] });
    const r = applyExceptions(contract(), exceptions, "texture");
    expect(r.problems).toHaveLength(1);
    expect(r.problems[0]).toContain("characters.halftoneC.opacity: matches nothing in contract.texture.json");
    const run = runCli(contract(), [ghost]);
    expect(run.status).toBe(1);
    expect(run.stdout + run.stderr).toContain("matches nothing in contract.texture.json");
  });

  it("case 4: an exception with an empty why or no decided date is unparseable and the run exits 1", () => {
    const noWhy = { ...base, why: "  " };
    const { decided: _decided, ...noDate } = base;
    for (const bad of [noWhy, noDate]) {
      const { exceptions, problems } = parseExceptions({ exceptions: [bad] });
      expect(exceptions).toEqual([]);
      expect(problems).toHaveLength(1);
      expect(problems[0]).toMatch(/unparseable/);
      expect(runCli(contract(), [bad]).status).toBe(1);
    }
  });

  it("case 5: two exceptions on the same property are a duplicate and the run exits 1", () => {
    const grain = { ...base, key: "characters.grain", winner: "0.4" };
    const { problems } = parseExceptions({ exceptions: [grain, { ...grain, why: "said twice" }] });
    expect(problems).toEqual(["texture characters.grain.opacity: duplicate exception"]);
    const texture = { characters: { grain: { opacity: "0.4" } } };
    expect(runCli(texture, [grain, { ...grain }]).status).toBe(1);
    expect(runCli(texture, [grain]).status).toBe(0);
  });
});

describe("the committed contract and overlay", () => {
  const read = (f) => readFileSync(join(here, f), "utf8");
  const texture = JSON.parse(read("contract.texture.json"));
  const overlay = parseExceptions(JSON.parse(read("exceptions.json")));

  it("case 6: the captured contract holds the winner's values and every exception names one of them", () => {
    expect(overlay.problems).toEqual([]);
    const ours = overlay.exceptions.filter((e) => e.target === "texture");
    expect(ours).toHaveLength(6);
    for (const e of ours) {
      const [page, layer] = e.key.split(".");
      expect(texture[page][layer][e.prop], `${e.key}.${e.prop}`).toBe(e.winner);
    }
    for (const page of ["characters", "segments"]) {
      expect(texture[page].halftoneA.opacity).toBe("0.55");
      expect(texture[page].halftoneB.opacity).toBe("0.35");
      expect(texture[page].grain.opacity).toBe("0.4");
    }
  });

  it("guard: the effective texture contract is byte-identical to the hand-edited one it replaces", () => {
    // sha256 of apps/console/fidelity/contract.texture.json at 3c8208f, the last hand-edited copy.
    const r = applyExceptions(texture, overlay.exceptions, "texture");
    expect(r.problems).toEqual([]);
    const bytes = JSON.stringify(r.contract, null, 2) + "\n";
    expect(createHash("sha256").update(bytes).digest("hex")).toBe(
      "1870dee1759a76185e542bd5307a1991354944760b4af3b357d1fbafca813853",
    );
  });

  it("case 7 (offline half): the summary names how many exceptions were waived, and each key", () => {
    const r = applyExceptions(texture, overlay.exceptions, "texture");
    const text = describeApplied(r.applied);
    expect(text).toMatch(/^6 owner exception\(s\) applied/);
    for (const e of overlay.exceptions) expect(text).toContain(`${e.target} ${e.key}.${e.prop}`);
    const cliRun = spawnSync(process.execPath, [cli], { encoding: "utf8" });
    expect(cliRun.status, cliRun.stdout + cliRun.stderr).toBe(0);
    expect(cliRun.stdout).toContain("6 owner exception(s) applied");
  });

  // Needs Python with Playwright, the linked contest skill and the git-ignored contest data; a
  // check run only (never `capture`), so it reads the winner nowhere and serves only localhost.
  const e2e = Boolean(process.env.SHADOWRUNR_CONTEST_DATA) && process.env.FIDELITY_E2E === "1";
  it.skipIf(!e2e)(
    "case 7: a full `node fidelity/run.mjs` names the owner exceptions it honoured",
    () => {
      const r = spawnSync(process.execPath, [join(here, "run.mjs")], {
        encoding: "utf8",
        env: { ...process.env, FIDELITY_REQUIRED: "1" },
      });
      const out = r.stdout + r.stderr;
      expect(r.status, out).toBe(0);
      expect(out).toContain("6 owner exception(s) applied");
      for (const e of overlay.exceptions) expect(out).toContain(`${e.target} ${e.key}.${e.prop}`);
    },
    600_000,
  );
});
