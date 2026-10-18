// The character package's markdown, read into blocks (never HTML): headings, paragraphs and
// bullet or numbered lists with one level of nesting, which is all the package files use. The
// typed readers below pull the constitution's red lines, a tone and a foil out of those blocks.

export interface ListItem {
  readonly text: string;
  readonly children: readonly ListItem[];
}

export type Block =
  | { readonly kind: "h"; readonly level: number; readonly text: string }
  | { readonly kind: "p"; readonly text: string }
  | { readonly kind: "ul" | "ol"; readonly items: readonly ListItem[] };

interface MutItem {
  text: string;
  children: MutItem[];
  indent: number;
}

const HEADING = /^(#{1,6})\s+(.*)$/;
const BULLET = /^(\s*)[-*]\s+(.*)$/;
const NUMBERED = /^(\s*)\d+[.)]\s+(.*)$/;

const freeze = (items: readonly MutItem[]): ListItem[] => items.map((i) => ({ text: i.text, children: freeze(i.children) }));

/** Parses package markdown into blocks. Unknown syntax stays as paragraph text. */
export function parseBlocks(src: string): Block[] {
  const out: Block[] = [];
  let para: string[] = [];
  let list: { kind: "ul" | "ol"; items: MutItem[] } | null = null;
  let last: MutItem | null = null;

  const flushPara = () => {
    if (para.length > 0) out.push({ kind: "p", text: para.join(" ") });
    para = [];
  };
  const flushList = () => {
    if (list) out.push({ kind: list.kind, items: freeze(list.items) });
    list = null;
    last = null;
  };

  for (const raw of src.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.replace(/\s+$/, "");
    if (line.trim().length === 0) {
      flushPara();
      // A blank line ends a list only when the next line is not another item (handled below).
      last = null;
      continue;
    }
    const h = HEADING.exec(line);
    if (h) {
      flushPara();
      flushList();
      out.push({ kind: "h", level: h[1]?.length ?? 1, text: (h[2] ?? "").trim() });
      continue;
    }
    const b = BULLET.exec(line);
    const n = b ? null : NUMBERED.exec(line);
    const m = b ?? n;
    if (m) {
      flushPara();
      const indent = (m[1] ?? "").length;
      const kind = b ? "ul" : "ol";
      const item: MutItem = { text: (m[2] ?? "").trim(), children: [], indent };
      const parent = list?.items[list.items.length - 1];
      if (list && parent && indent >= parent.indent + 2) {
        parent.children.push(item);
      } else {
        if (list && list.kind !== kind) flushList();
        list ??= { kind, items: [] };
        list.items.push(item);
      }
      last = item;
      continue;
    }
    if (last && /^\s+/.test(raw)) {
      // A continuation line of the current list item.
      last.text = `${last.text} ${line.trim()}`;
      continue;
    }
    flushList();
    para.push(line.trim());
  }
  flushPara();
  flushList();
  return out;
}

export interface DocSection {
  readonly title: string;
  readonly blocks: readonly Block[];
}

export interface Doc {
  /** The level-1 heading, when there is one. */
  readonly title: string | undefined;
  /** Blocks between the title and the first level-2 heading. */
  readonly preamble: readonly Block[];
  readonly sections: readonly DocSection[];
}

/** Splits blocks at level-2 headings. */
export function parseDoc(src: string): Doc {
  let title: string | undefined;
  const preamble: Block[] = [];
  const sections: { title: string; blocks: Block[] }[] = [];
  for (const b of parseBlocks(src)) {
    if (b.kind === "h" && b.level === 1 && title === undefined && sections.length === 0 && preamble.length === 0) {
      title = b.text;
    } else if (b.kind === "h" && b.level === 2) {
      sections.push({ title: b.text, blocks: [] });
    } else {
      (sections[sections.length - 1]?.blocks ?? preamble).push(b);
    }
  }
  return { title, preamble, sections };
}

const firstPara = (blocks: readonly Block[]): string | undefined => {
  const p = blocks.find((b) => b.kind === "p");
  return p?.kind === "p" ? p.text : undefined;
};

const listItems = (blocks: readonly Block[]): ListItem[] => blocks.flatMap((b) => (b.kind === "ul" || b.kind === "ol" ? [...b.items] : []));

/** `Ratio: about 60%` → the text after the label, for the first item that starts with it. */
function labelled(items: readonly ListItem[], label: string): ListItem | undefined {
  return items.find((i) => i.text.toLowerCase().startsWith(`${label.toLowerCase()}:`));
}

const afterLabel = (i: ListItem): string => i.text.slice(i.text.indexOf(":") + 1).trim();

const sectionOf = (doc: Doc, re: RegExp): DocSection | undefined => doc.sections.find((s) => re.test(s.title));

const paras = (blocks: readonly Block[]): string =>
  blocks
    .filter((b) => b.kind === "p")
    .map((b) => (b.kind === "p" ? b.text : ""))
    .join(" ");

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
