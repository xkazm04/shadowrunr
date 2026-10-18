// The character package's documents read into typed shapes: the constitution's red lines, a
// tone and a foil, over the safe-markdown blocks of `ui/doc`.
import { afterLabel, firstPara, labelled, listItems, paras, parseBlocks, parseDoc, sectionOf, type Block, type DocSection } from "../../ui/doc";

// ---- the constitution ----

export interface RedLine {
  readonly n: number;
  /** The rule's head, up to its first colon or full stop. */
  readonly short: string;
  /** The rest of the rule. */
  readonly text: string;
}

export interface Constitution {
  readonly title: string | undefined;
  readonly preamble: readonly Block[];
  readonly redLines: readonly RedLine[];
  /** The red lines heading's aside: `never crossed, in any tone, under any vote`. */
  readonly redLinesNote: string | undefined;
  readonly sections: readonly DocSection[];
}

export function splitRule(text: string): { short: string; text: string } {
  const m = /^(.+?)([:.])\s+(.*)$/.exec(text);
  if (!m) return { short: text, text: "" };
  return { short: m[2] === "." ? `${m[1] ?? ""}.` : (m[1] ?? text), text: m[3] ?? "" };
}

export function readConstitution(src: string): Constitution {
  const doc = parseDoc(src);
  const red = sectionOf(doc, /^red lines/i);
  const note = red ? /\(([^)]*)\)/.exec(red.title)?.[1] : undefined;
  return {
    title: doc.title,
    preamble: doc.preamble,
    redLines: red ? listItems(red.blocks).map((i, k) => ({ n: k + 1, ...splitRule(i.text) })) : [],
    redLinesNote: note,
    sections: doc.sections.filter((s) => s !== red),
  };
}

// ---- tones ----

export interface ToneDoc {
  readonly id: string;
  readonly title: string;
  /** The `(baseline)` marker on the heading. */
  readonly baseline: boolean;
  readonly desc: string | undefined;
  readonly ratio: string | undefined;
  readonly edge: string | undefined;
  readonly moves: readonly string[];
  readonly examples: readonly string[];
  readonly bet: string | undefined;
}

export function readTone(id: string, src: string): ToneDoc {
  const doc = parseDoc(src);
  const head = (doc.title ?? id).replace(/^Tone:\s*/i, "");
  const items = listItems(doc.preamble);
  const ratio = labelled(items, "Ratio");
  const edge = labelled(items, "Edge");
  const moves = labelled(items, "Signature moves");
  const examples = sectionOf(doc, /^example lines/i);
  const bet = sectionOf(doc, /^the bet/i);
  const movesInline = moves ? afterLabel(moves) : "";
  return {
    id,
    title: head.replace(/\s*\(baseline\)\s*$/i, ""),
    baseline: /\(baseline\)/i.test(head),
    desc: firstPara(doc.preamble),
    ratio: ratio ? afterLabel(ratio) : undefined,
    edge: edge ? afterLabel(edge) : undefined,
    moves: moves ? (moves.children.length > 0 ? moves.children.map((c) => c.text) : movesInline.length > 0 ? [movesInline] : []) : [],
    examples: examples ? listItems(examples.blocks).map((i) => i.text.replace(/^"(.*)"$/, "$1")) : [],
    bet: bet ? paras(bet.blocks) || undefined : undefined,
  };
}

// ---- foils ----

export interface FoilDoc {
  readonly desc: string | undefined;
  readonly fn: string | undefined;
  readonly bits: string | undefined;
  readonly redLines: string | undefined;
}

export function readFoil(src: string): FoilDoc {
  const doc = parseDoc(src);
  const items = listItems(doc.preamble);
  const pick = (label: string) => {
    const i = labelled(items, label);
    return i ? afterLabel(i) : undefined;
  };
  return { desc: firstPara(doc.preamble), fn: pick("Function"), bits: pick("Signature bits"), redLines: pick("Red lines") };
}

/** A rule file's body without its heading (`rules/<id>.md`). */
export function ruleBody(src: string): Block[] {
  return parseBlocks(src).filter((b) => !(b.kind === "h" && b.level === 1));
}
