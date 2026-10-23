// Characters: the cast, one package summarised, whole package documents (character.doc.get and
// .set, docs/spec/contract.md "athena copilot"), validation, and the GATED character writes.
import type { LineSituation, LineStatus } from "@shadowrunr/contract";
import { allRoute, pkgRoute, type PkgTab } from "../../pages/characters/routes";
import { encodeRoute, pageRoute } from "../../router";
import { argDocOf as argDoc, changedChars, docPage, docPath, DOCS, occurrences, PAGE_CHARS } from "./docs";
import { clip } from "./summary";
import { argInt, argStr, bounded, fail, fitBody, int, obj, RESULT_BUDGET, SHOW, str, withRoomFor, type ToolSpec } from "./tool";

/** The package tab that shows a document. */
export function tabOf(doc: string): PkgTab {
  if (doc === "constitution.md") return "constitution";
  if (doc === "bible.md") return "bible";
  if (doc === "directions.md") return "tone-lab";
  if (doc === "situations.json") return "situations";
  if (doc.startsWith("foils/")) return "foils";
  return "parameters";
}

const ID = str("A character id, e.g. malakor (list_characters).");
const DOC_ARG = str(`The document: ${DOCS}.`, { maxLength: 64 });
const DIGEST = str("The digest read_character_doc returned.", { maxLength: 80 });
const SITUATIONS: readonly LineSituation[] = ["roast_request", "bait", "newcomer", "vote_loss", "gift", "ai_question", "quiet", "research_wait", "proposal", "patron", "flood", "general"];
const STATUSES: readonly LineStatus[] = ["approved", "seed", "retired"];
const withView = (body: Record<string, unknown>, view: string | undefined) => (view === undefined ? body : { ...body, view });

export const CHARACTER_TOOLS: readonly ToolSpec[] = [
  {
    name: "list_characters",
    title: "List characters",
    description: "Returns every character package: id, name, tagline, active and default tone, tones, foils and optional rules on of total. Shows #/characters/all. Then get_character{id}.",
    cls: "READ",
    input: obj({ show: SHOW }),
    run: async (input, ctx) => {
      const rows = await ctx.read("character.list", {});
      const items = rows.map((c) => ({
        id: c.id,
        name: c.name,
        ...(c.tagline === undefined ? {} : { tagline: c.tagline }),
        active_tone: c.active_tone,
        default_tone: c.default_tone,
        tones: c.tones,
        foils: c.foils,
        rules: `${c.rules_enabled} of ${c.rules_total} on`,
      }));
      return withView({ ...bounded(items, items.length) }, ctx.show(input, allRoute()));
    },
  },
  {
    name: "get_character",
    title: "Summarise a character",
    description: "Returns a package summarised, not its full text: name, tones, foils, rules on/off, situations, files, and the documents read_character_doc can open. Shows #/characters/<id>/package.",
    cls: "READ",
    input: obj({ id: ID, show: SHOW }, ["id"]),
    run: async (input, ctx) => {
      const id = argStr(input, "id");
      const c = await ctx.read("character.get", { id });
      // The names read_character_doc takes, as bare stems.
      const docs = ["constitution", "bible", "directions", "character", "stalls", "situations", ...c.foils.map((f) => `foils/${f.id}`)];
      const body = {
        id: c.id,
        name: c.name,
        ...(c.tagline === undefined ? {} : { tagline: clip(c.tagline, 200) }),
        active_tone: c.active_tone,
        default_tone: c.default_tone,
        tones: c.tones.map((t) => t.id),
        foils: c.foils.map((f) => ({ id: f.id, declared: f.declared })),
        rules: c.rules.map((r) => ({ id: r.id, title: r.title, enabled: r.enabled })),
        situations: bounded(c.situations.map((s) => s.id), 20),
        files: bounded(c.files.map((f) => `${f.path} (${f.bytes} B)`), 40),
        docs: docs.filter((d) => docPath(d) !== undefined),
      };
      // The constitution's opening takes the room the rest leaves (up to 400 chars).
      const view = ctx.show(input, pageRoute("characters", id, "package"));
      return withView(withRoomFor(fitBody(body, RESULT_BUDGET - 80) ?? body, "constitution_preview", c.constitution, 400, RESULT_BUDGET - 80), view);
    },
  },
  {
    name: "read_character_doc",
    title: "Read a package document",
    description:
      "Returns one page of a package document (about 1,200 chars, on a line) with its digest, total_chars and next_offset; the first page adds the outline, headings with offsets. Pass offset to read on. Shows its package tab.",
    cls: "READ",
    input: obj({ id: ID, doc: DOC_ARG, offset: int("The char offset to read from (default 0): next_offset, or an outline offset.", 0), limit: int(`The most chars of text (default and at most ${PAGE_CHARS}).`, 100, PAGE_CHARS), show: SHOW }, ["id", "doc"]),
    run: async (input, ctx) => {
      const id = argStr(input, "id");
      const doc = argDoc(input);
      const d = await ctx.read("character.doc.get", { id, doc });
      const offset = argInt(input, "offset", 0, 0, Number.MAX_SAFE_INTEGER);
      if (offset > d.text.length) fail("invalid_params", `offset ${offset} is past the end (${d.text.length} chars)`);
      const view = ctx.show(input, pkgRoute(id, tabOf(doc)));
      const chars = argInt(input, "limit", PAGE_CHARS, 100, PAGE_CHARS);
      return docPage({ id: d.id, doc: d.doc, digest: d.digest, ...(view === undefined ? {} : { view }) }, d.text, offset, chars);
    },
  },
  {
    name: "validate_character",
    title: "Validate a package",
    description: "Validates a character package and returns its issues (file, key, reason, severity) with error and warning counts. Bounded. Shows #/characters/<id>/package.",
    cls: "READ",
    input: obj({ id: ID, show: SHOW }, ["id"]),
    run: async (input, ctx) => {
      const id = argStr(input, "id");
      const issues = await ctx.read("character.validate", { id });
      const errors = issues.filter((i) => i.severity === "error").length;
      return withView(
        { id, valid: errors === 0, errors, warnings: issues.length - errors, ...bounded(issues, 20) },
        ctx.show(input, pageRoute("characters", id, "package")),
      );
    },
  },
  {
    name: "set_character_doc",
    title: "Replace a package document",
    description:
      "Replaces a package document whole: for a new foil (base_digest '') or a small file. To change a long document use edit_character_doc. A stale digest is refused as conflict, an invalid package is undone. Returns digests.",
    cls: "GATED",
    input: obj(
      { id: ID, doc: DOC_ARG, text: str("The whole new text of the document."), base_digest: str("The digest from read_character_doc, or '' for a new file.") },
      ["id", "doc", "text", "base_digest"],
    ),
    run: async (input, ctx) => {
      const id = argStr(input, "id");
      const doc = argDoc(input);
      const text = input["text"];
      const base = input["base_digest"];
      if (typeof text !== "string" || typeof base !== "string") fail("invalid_params", "text and base_digest must be strings");
      const r = await ctx.write("character.doc.set", { id, doc, text, base_digest: base });
      return { ...r, see: encodeRoute(pkgRoute(id, tabOf(doc))) };
    },
  },
  {
    name: "edit_character_doc",
    title: "Edit a package document",
    description:
      "The way to change a long document: replaces find, exact text that occurs once, with replace. To insert, find an anchor sentence and replace it with anchor plus new text. base_digest is the digest read. Returns new digest, backup.",
    cls: "GATED",
    input: obj(
      {
        id: ID,
        doc: DOC_ARG,
        base_digest: DIGEST,
        find: str("The exact text to replace; it must occur exactly once.", { minLength: 1, maxLength: 4000 }),
        replace: str("The text that takes its place ('' deletes it).", { maxLength: 8000 }),
      },
      ["id", "doc", "base_digest", "find", "replace"],
    ),
    run: async (input, ctx) => {
      const id = argStr(input, "id");
      const doc = argDoc(input);
      const base = argStr(input, "base_digest");
      const find = argStr(input, "find");
      const replace = input["replace"];
      if (typeof replace !== "string") fail("invalid_params", "replace must be a string ('' deletes find)");
      if (replace === find) fail("invalid_params", "replace is the same as find; nothing would change");
      const d = await ctx.read("character.doc.get", { id, doc });
      if (d.digest !== base) fail("conflict", `${doc} changed since ${base} (it is now ${d.digest}); read it again with read_character_doc and redo the edit`);
      const at = occurrences(d.text, find);
      if (at.length !== 1) {
        fail("invalid_params", at.length === 0 ? `find occurs 0 times in ${doc}; copy the exact text from read_character_doc` : `find occurs ${at.length} times in ${doc}; add surrounding text so it occurs exactly once`);
      }
      const i = at[0] ?? 0;
      const text = `${d.text.slice(0, i)}${replace}${d.text.slice(i + find.length)}`;
      const r = await ctx.write("character.doc.set", { id, doc, text, base_digest: base });
      return {
        doc: r.doc,
        digest: r.digest,
        previous_digest: r.previous_digest,
        ...(r.backup === undefined ? {} : { backup: r.backup }),
        changed_chars: changedChars(find, replace),
        see: encodeRoute(pkgRoute(id, tabOf(doc))),
      };
    },
  },
  {
    name: "set_character_rules",
    title: "Switch an optional rule",
    description: "Switches one optional constitution rule on or off for a character (the operator's override; a rail stays on). Returns the rule list after the write.",
    cls: "GATED",
    input: obj({ character: ID, id: str("The rule id (get_character rules)."), enabled: { type: "boolean" } }, ["character", "id", "enabled"]),
    run: async (input, ctx) => {
      const enabled = input["enabled"];
      if (typeof enabled !== "boolean") fail("invalid_params", "enabled must be a boolean");
      const r = await ctx.write("character.rules.set", { character: argStr(input, "character"), id: argStr(input, "id"), enabled });
      return { result: r };
    },
  },
  {
    name: "set_tone",
    title: "Set the active tone",
    description: "Makes one tone variant the character's active tone for every stream (the operator's choice; default_tone stays in the package). Returns the character summary.",
    cls: "GATED",
    input: obj({ id: ID, tone: str("A tone variant id (get_character tones).") }, ["id", "tone"]),
    run: async (input, ctx) => {
      const c = await ctx.write("character.tone.set", { id: argStr(input, "id"), tone: argStr(input, "tone") });
      return { id: c.id, active_tone: c.active_tone, default_tone: c.default_tone };
    },
  },
  {
    name: "add_tone_line",
    title: "Add a line to a tone bank",
    description: "Appends a line to a tone's bank (lines/<tone>.toml) tagged with one or more situations, approved unless status says seed. The package is validated after. Returns the line and file.",
    cls: "GATED",
    input: obj(
      {
        character: ID,
        tone: str("The tone variant id."),
        text: str("The line as the character says it."),
        tags: { type: "array", items: { type: "string", enum: SITUATIONS }, minItems: 1, maxItems: SITUATIONS.length, description: "Situations the line fits." },
        status: { type: "string", enum: STATUSES },
      },
      ["character", "tone", "text", "tags"],
    ),
    run: async (input, ctx) => {
      const tags = input["tags"];
      if (!Array.isArray(tags) || tags.length === 0 || !tags.every((t): t is LineSituation => SITUATIONS.includes(t))) fail("invalid_params", "tags must be one or more situations");
      const status = input["status"];
      const s = STATUSES.find((x) => x === status);
      if (status !== undefined && s === undefined) fail("invalid_params", `status must be one of ${STATUSES.join(", ")}`);
      const r = await ctx.write("tone.lines.add", {
        character: argStr(input, "character"),
        tone: argStr(input, "tone"),
        text: argStr(input, "text"),
        tags,
        ...(s === undefined ? {} : { status: s }),
      });
      return { result: r };
    },
  },
  {
    name: "set_tone_line_status",
    title: "Set a bank line's status",
    description: "Sets one bank line's status: approved, seed or retired (a retired line is never shown to the model). Line ids come from query tone.lines.list. Returns the line and file.",
    cls: "GATED",
    input: obj(
      { character: ID, tone: str("The tone variant id."), id: str("The line id (tone.lines.list)."), status: { type: "string", enum: STATUSES } },
      ["character", "tone", "id", "status"],
    ),
    run: async (input, ctx) => {
      const status = STATUSES.find((x) => x === input["status"]);
      if (status === undefined) fail("invalid_params", `status must be one of ${STATUSES.join(", ")}`);
      const r = await ctx.write("tone.lines.set_status", { character: argStr(input, "character"), tone: argStr(input, "tone"), id: argStr(input, "id"), status });
      return { result: r };
    },
  },
];
