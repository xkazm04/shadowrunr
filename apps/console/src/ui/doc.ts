// Safe markdown: package and report prose read into blocks (never HTML), rendered by
// `Markdown.tsx` as React elements. Headings, paragraphs, and bullet or numbered lists with one
// level of nesting; anything else stays paragraph text. Typed readers (a character's
// constitution, tones, foils) build on the block helpers below.

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

export const firstPara = (blocks: readonly Block[]): string | undefined => {
  const p = blocks.find((b) => b.kind === "p");
  return p?.kind === "p" ? p.text : undefined;
};

export const listItems = (blocks: readonly Block[]): ListItem[] => blocks.flatMap((b) => (b.kind === "ul" || b.kind === "ol" ? [...b.items] : []));

/** `Ratio: about 60%` → the text after the label, for the first item that starts with it. */
export function labelled(items: readonly ListItem[], label: string): ListItem | undefined {
  return items.find((i) => i.text.toLowerCase().startsWith(`${label.toLowerCase()}:`));
}

export const afterLabel = (i: ListItem): string => i.text.slice(i.text.indexOf(":") + 1).trim();

export const sectionOf = (doc: Doc, re: RegExp): DocSection | undefined => doc.sections.find((s) => re.test(s.title));

export const paras = (blocks: readonly Block[]): string =>
  blocks
    .filter((b) => b.kind === "p")
    .map((b) => (b.kind === "p" ? b.text : ""))
    .join(" ");
