// Package documents for the agent: the doc names Athena may pass (bare stems accepted), a page
// of a long document cut to fit the result budget with its outline, and the exact-once find
// and replace that edit_character_doc files. Pure; the tools do the IO.
import { fail, RESULT_BUDGET, sizeOf } from "./tool";

/** The documents character.doc.get/set accept (the daemon checks the same list). */
export const DOC = /^(constitution\.md|bible\.md|directions\.md|character\.toml|stalls\.toml|situations\.json|(foils|private)\/[a-z0-9_-]{1,40}\.md)$/;
export const DOCS = "constitution, bible, directions, character, stalls, situations (extension optional), foils/<slug> or private/<slug>";

/** The six named documents by their bare stem. */
const STEMS: Readonly<Record<string, string>> = {
  constitution: "constitution.md",
  bible: "bible.md",
  directions: "directions.md",
  character: "character.toml",
  stalls: "stalls.toml",
  situations: "situations.json",
};

/** A doc name as Athena may write it, as the daemon's package-relative path; undefined if not one. */
export function docPath(raw: string): string | undefined {
  const s = raw.trim();
  const doc = STEMS[s] ?? (/^(foils|private)\/[a-z0-9_-]{1,40}$/.test(s) ? `${s}.md` : s);
  return DOC.test(doc) ? doc : undefined;
}

export function argDocOf(input: Record<string, unknown>): string {
  const raw = input["doc"];
  const doc = typeof raw === "string" ? docPath(raw) : undefined;
  if (doc === undefined) fail("invalid_params", `doc ${JSON.stringify(raw ?? "")} is not a package document; use ${DOCS}`);
  return doc;
}

/** The most text one page carries, in chars of the document. */
export const PAGE_CHARS = 1200;
const OUTLINE_MAX = 20;
/** A first page keeps its outline only while the text still gets at least this many chars. */
const MIN_TEXT_WITH_OUTLINE = 600;

/** The markdown headings with the char offset each starts at, at most OUTLINE_MAX, clipped. */
export function outlineOf(text: string): string[] {
  const out: string[] = [];
  const re = /^#{1,6} .*$/gm;
  for (let m = re.exec(text); m !== null && out.length < OUTLINE_MAX; m = re.exec(text)) {
    const h = m[0].length > 64 ? `${m[0].slice(0, 63)}…` : m[0];
    out.push(`${m.index} ${h}`);
  }
  return out;
}

/** The end of a page from `offset` whose JSON string fits `room` chars, cut on a line where possible. */
function pageEnd(text: string, offset: number, room: number, chars: number): number {
  let end = Math.min(text.length, offset + chars);
  while (end > offset && sizeOf(text.slice(offset, end)) > room) end -= Math.max(1, Math.ceil((end - offset) / 20));
  if (end >= text.length) return end;
  const nl = text.lastIndexOf("\n", end - 1);
  return nl >= offset + (end - offset) / 2 ? nl + 1 : end;
}

/** One page of a document: the text from `offset` and where the next page starts. */
export function docPage(head: Record<string, unknown>, text: string, offset: number, chars = PAGE_CHARS): Record<string, unknown> {
  const base = { ...head, total_chars: text.length, offset };
  const shape = (end: number, outline?: string[]) => ({
    ...base,
    ...(end < text.length ? { next_offset: end } : {}),
    ...(outline === undefined || outline.length === 0 ? {} : { outline }),
    text: text.slice(offset, end),
  });
  // The room left for the text is what the budget leaves after everything else in the answer.
  const roomBeside = (outline?: string[]) => RESULT_BUDGET - sizeOf({ ...shape(offset, outline), next_offset: text.length, text: "" }) + 2;
  let outline = offset === 0 ? outlineOf(text) : undefined;
  if (outline !== undefined && roomBeside(outline) < MIN_TEXT_WITH_OUTLINE + 2) {
    // Drop headings from the end until the text keeps its share, else drop the outline.
    while (outline.length > 0 && roomBeside(outline) < MIN_TEXT_WITH_OUTLINE + 2) outline = outline.slice(0, -1);
    if (outline.length === 0) outline = undefined;
  }
  return shape(pageEnd(text, offset, roomBeside(outline), chars), outline);
}

/** Every start of `find` in `text`, overlapping ones included. */
export function occurrences(text: string, find: string): number[] {
  const at: number[] = [];
  for (let i = text.indexOf(find); i >= 0; i = text.indexOf(find, i + 1)) at.push(i);
  return at;
}

/** The chars a replacement changes: the longer side less what the two share at either end. */
export function changedChars(find: string, replace: string): number {
  let pre = 0;
  while (pre < find.length && pre < replace.length && find[pre] === replace[pre]) pre++;
  let suf = 0;
  while (suf < find.length - pre && suf < replace.length - pre && find[find.length - 1 - suf] === replace[replace.length - 1 - suf]) suf++;
  return Math.max(find.length, replace.length) - pre - suf;
}
