// Characters: the cast, one package summarised, whole package documents (character.doc.get and
// .set, docs/spec/contract.md "athena copilot"), validation, and the GATED character writes.
import type { LineSituation, LineStatus } from "@shadowrunr/contract";
import { allRoute, pkgRoute, type PkgTab } from "../../pages/characters/routes";
import { encodeRoute, pageRoute } from "../../router";
import { clip } from "./summary";
import { argStr, bounded, fail, obj, SHOW, str, type ToolSpec } from "./tool";

/** The documents character.doc.get/set accept (the daemon checks the same list). */
const DOC = /^(constitution\.md|bible\.md|directions\.md|character\.toml|stalls\.toml|situations\.json|foils\/[a-z0-9_-]{1,40}\.md)$/;
const DOCS = "constitution.md, bible.md, directions.md, character.toml, stalls.toml, situations.json or foils/<slug>.md";

/** The package tab that shows a document. */
export function tabOf(doc: string): PkgTab {
  if (doc === "constitution.md") return "constitution";
  if (doc === "bible.md") return "bible";
  if (doc === "directions.md") return "tone-lab";
  if (doc === "situations.json") return "situations";
  if (doc.startsWith("foils/")) return "foils";
  return "parameters";
}

function argDoc(input: Record<string, unknown>): string {
  const doc = argStr(input, "doc");
  if (!DOC.test(doc)) fail("invalid_params", `doc "${doc}" is not a package document; use ${DOCS}`);
  return doc;
}

const ID = str("A character id, e.g. malakor (list_characters).");
const DOC_ARG = str(`The package-relative document: ${DOCS}.`, { maxLength: 64 });
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
      const docs = ["constitution.md", "bible.md", "directions.md", "character.toml", "stalls.toml", "situations.json", ...c.foils.map((f) => `foils/${f.id}.md`)];
      return withView(
        {
          id: c.id,
          name: c.name,
          ...(c.tagline === undefined ? {} : { tagline: c.tagline }),
          active_tone: c.active_tone,
          default_tone: c.default_tone,
          tones: c.tones.map((t) => t.id),
          foils: c.foils.map((f) => ({ id: f.id, declared: f.declared })),
          rules: c.rules.map((r) => ({ id: r.id, title: r.title, enabled: r.enabled })),
          situations: bounded(c.situations.map((s) => s.id), 20),
          constitution_preview: clip(c.constitution, 400),
          files: c.files.map((f) => `${f.path} (${f.bytes} B)`),
          docs: docs.filter((d) => DOC.test(d)),
        },
        ctx.show(input, pageRoute("characters", id, "package")),
      );
    },
  },
  {
    name: "read_character_doc",
    title: "Read a package document",
    description: "Returns a package document's whole text and its digest; pass that digest as base_digest to set_character_doc. Shows the package tab for the document.",
    cls: "READ",
    input: obj({ id: ID, doc: DOC_ARG, show: SHOW }, ["id", "doc"]),
    run: async (input, ctx) => {
      const id = argStr(input, "id");
      const doc = argDoc(input);
      const d = await ctx.read("character.doc.get", { id, doc });
      return withView({ id: d.id, doc: d.doc, digest: d.digest, chars: d.text.length, text: d.text }, ctx.show(input, pkgRoute(id, tabOf(doc))));
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
    description: "Replaces a package document whole. base_digest is the digest read_character_doc returned ('' for a new foil); a stale digest is refused as conflict, an invalid package is undone. Returns digests and the backup.",
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
