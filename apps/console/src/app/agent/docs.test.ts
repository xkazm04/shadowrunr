// Package documents through the agent: bare doc stems, a long document paged within the result
// budget with its outline, and edit_character_doc's digest check and exact-once find/replace.
import { describe, expect, it } from "vitest";
import { malakorDetail } from "../../pages/characters/testgen";
import { buildTools } from "./catalog";
import { changedChars, docPath, outlineOf, PAGE_CHARS } from "./docs";
import { fakes, no, ok, run, type Script } from "./testkit";
import { RESULT_BUDGET } from "./tool";

const BIBLE = malakorDetail().bible ?? "";
const DIGEST = `sha256:${"ab".repeat(32)}`;

function setup(script: Script) {
  const f = fakes(script);
  const tools = new Map(buildTools(f.deps).map((t) => [t.name, t]));
  return { ...f, tool: (name: string) => tools.get(name) };
}

const docGet = (text: string, digest = DIGEST): Script[string] => (p) => ok({ ...(p as { id: string; doc: string }), text, digest });

describe("doc names", () => {
  it("accepts the bare stems of the six named documents and foils/<slug>, with or without the extension", () => {
    expect(["bible", "constitution", "directions", "character", "stalls", "situations"].map(docPath)).toEqual(["bible.md", "constitution.md", "directions.md", "character.toml", "stalls.toml", "situations.json"]);
    expect(docPath("bible.md")).toBe("bible.md");
    expect(docPath("foils/the-inspector")).toBe("foils/the-inspector.md");
    expect(docPath("foils/the-inspector.md")).toBe("foils/the-inspector.md");
    for (const bad of ["../x", "tones/theatrical", "foils/../bible", "lines/theatrical.toml", ""]) expect(docPath(bad), bad).toBeUndefined();
  });

  it("sends the stem as the daemon's path, and refuses anything else with the legal values", async () => {
    const s = setup({ "character.doc.get": docGet("# Constitution") });
    await run(s.tool("read_character_doc"), { id: "malakor", doc: "constitution", show: false });
    await expect(run(s.tool("read_character_doc"), { id: "malakor", doc: "tones/theatrical" })).rejects.toThrow(/^invalid_params: doc "tones\/theatrical" is not a package document; use constitution, bible, directions, character, stalls, situations \(extension optional\) or foils\/<slug>/);
    expect(s.sent).toEqual(["character.doc.get"]);
  });
});

describe("read_character_doc", () => {
  it("pages a 13k-char bible within the budget: the first page has the outline, the pages join back to the whole text", async () => {
    expect(BIBLE.length).toBeGreaterThan(13_000);
    const s = setup({ "character.doc.get": docGet(BIBLE) });
    const pages: Record<string, unknown>[] = [];
    let offset: number | undefined = 0;
    while (offset !== undefined && pages.length < 40) {
      const tool = s.tool("read_character_doc");
      const text = (await tool?.execute({ id: "malakor", doc: "bible", offset }))?.content[0]?.text ?? "";
      expect(text.length).toBeLessThanOrEqual(RESULT_BUDGET);
      const page = await run(tool, { id: "malakor", doc: "bible", offset });
      pages.push(page);
      expect((page["text"] as string).length).toBeLessThanOrEqual(PAGE_CHARS);
      offset = page["next_offset"] as number | undefined;
    }
    expect(pages.map((p) => p["text"]).join("")).toBe(BIBLE);
    expect(pages[0]).toMatchObject({ doc: "bible.md", digest: DIGEST, total_chars: BIBLE.length, offset: 0, view: "#/characters/malakor/package/bible" });
    expect(pages[0]?.["outline"]).toEqual(outlineOf(BIBLE));
    expect(pages[1]).not.toHaveProperty("outline");
    // Every page but the last ends on a line.
    for (const p of pages.slice(0, -1)) expect(p["text"] as string).toMatch(/\n$/);
  });

  it("jumps to an outline offset, and refuses an offset past the end", async () => {
    const s = setup({ "character.doc.get": docGet(BIBLE) });
    const first = await run(s.tool("read_character_doc"), { id: "malakor", doc: "bible", show: false });
    const jokes = (first["outline"] as string[]).find((h) => h.includes("Running jokes")) ?? "";
    const at = Number(jokes.split(" ")[0]);
    const page = await run(s.tool("read_character_doc"), { id: "malakor", doc: "bible", offset: at, limit: 300, show: false });
    expect(page["text"]).toMatch(/^## Running jokes/);
    expect((page["text"] as string).length).toBeLessThanOrEqual(300);
    await expect(run(s.tool("read_character_doc"), { id: "malakor", doc: "bible", offset: BIBLE.length + 1 })).rejects.toThrow(/past the end/);
  });

  it("returns a short document whole, with no next_offset", async () => {
    const s = setup({ "character.doc.get": docGet("# The Inspector\n\nA foil.") });
    const body = await run(s.tool("read_character_doc"), { id: "malakor", doc: "foils/the-inspector" });
    expect(body).toMatchObject({ doc: "foils/the-inspector.md", text: "# The Inspector\n\nA foil.", total_chars: 24, view: "#/characters/malakor/package/foils" });
    expect(body).not.toHaveProperty("next_offset");
  });
});

describe("edit_character_doc", () => {
  const anchor = "## Running jokes";
  const written = (s: ReturnType<typeof setup>) => s.writes.map((w) => w.params as { text: string; base_digest: string; doc: string });

  it("replaces the one occurrence, writes the whole text against the digest read, and reports what changed", async () => {
    const s = setup({
      "character.doc.get": docGet(BIBLE),
      "character.doc.set": () => ok({ id: "malakor", doc: "bible.md", digest: "sha256:new", previous_digest: DIGEST, backup: "character-backups/malakor/bible.md.1" }),
    });
    const insert = `${anchor}\n\n- He audits one account at a time.`;
    const body = await run(s.tool("edit_character_doc"), { id: "malakor", doc: "bible", base_digest: DIGEST, find: anchor, replace: insert });
    expect(written(s)).toEqual([{ id: "malakor", doc: "bible.md", base_digest: DIGEST, text: BIBLE.replace(anchor, insert) }]);
    expect(body).toEqual({ doc: "bible.md", digest: "sha256:new", previous_digest: DIGEST, backup: "character-backups/malakor/bible.md.1", changed_chars: insert.length - anchor.length, see: "#/characters/malakor/package/bible" });
  });

  it("refuses a stale base_digest as conflict and writes nothing", async () => {
    const s = setup({ "character.doc.get": docGet(BIBLE, "sha256:moved"), "character.doc.set": () => ok({}) });
    await expect(run(s.tool("edit_character_doc"), { id: "malakor", doc: "bible", base_digest: DIGEST, find: anchor, replace: "x" })).rejects.toThrow(/^conflict: bible\.md changed since .*read it again with read_character_doc/);
    expect(s.writes).toEqual([]);
  });

  it("refuses find occurring 0 or more than once, with the count, and writes nothing", async () => {
    const s = setup({ "character.doc.get": docGet("one two one"), "character.doc.set": () => ok({}) });
    await expect(run(s.tool("edit_character_doc"), { id: "malakor", doc: "bible", base_digest: DIGEST, find: "three", replace: "x" })).rejects.toThrow(/^invalid_params: find occurs 0 times in bible\.md/);
    await expect(run(s.tool("edit_character_doc"), { id: "malakor", doc: "bible", base_digest: DIGEST, find: "one", replace: "x" })).rejects.toThrow(/^invalid_params: find occurs 2 times in bible\.md; add surrounding text/);
    expect(s.writes).toEqual([]);
  });

  it("carries the daemon's refusal of a write that breaks the package", async () => {
    const s = setup({ "character.doc.get": docGet("a b"), "character.doc.set": () => no("invalid_params", "the package no longer validates", { issues: [{ file: "bible.md", reason: "x", severity: "error" }] }) });
    await expect(run(s.tool("edit_character_doc"), { id: "malakor", doc: "bible", base_digest: DIGEST, find: "b", replace: "c" })).rejects.toThrow(/^invalid_params: the package no longer validates Issues:/);
  });

  it("counts the chars a replacement changes, ignoring what the two sides share", () => {
    expect(changedChars("abc", "abc def")).toBe(4);
    expect(changedChars("the cat sat", "the dog sat")).toBe(3);
    expect(changedChars("remove me", "")).toBe(9);
  });
});
