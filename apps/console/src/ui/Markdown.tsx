// Renders package markdown blocks as React elements (no HTML strings): `code`, **strong** and
// *emphasis* inline, headings shifted so a document's own `##` sits under the page's section.
import type { ReactNode } from "react";
import type { Block, ListItem } from "./doc";
import styles from "./Markdown.module.css";

const TOKEN = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*\s][^*]*\*)/g;

/** Inline markdown → React nodes. */
export function Inline({ text }: { readonly text: string }) {
  const out: ReactNode[] = [];
  let at = 0;
  let k = 0;
  for (const m of text.matchAll(TOKEN)) {
    const i = m.index;
    if (i > at) out.push(text.slice(at, i));
    const t = m[0];
    if (t.startsWith("`")) out.push(<code key={k++}>{t.slice(1, -1)}</code>);
    else if (t.startsWith("**")) out.push(<strong key={k++}>{t.slice(2, -2)}</strong>);
    else out.push(<em key={k++}>{t.slice(1, -1)}</em>);
    at = i + t.length;
  }
  if (at < text.length) out.push(text.slice(at));
  return <>{out}</>;
}

function Items({ items, ordered }: { readonly items: readonly ListItem[]; readonly ordered: boolean }) {
  const body = items.map((it, i) => (
    <li key={i}>
      <Inline text={it.text} />
      {it.children.length > 0 && <Items items={it.children} ordered={false} />}
    </li>
  ));
  return ordered ? <ol className={styles.ol}>{body}</ol> : <ul className={styles.ul}>{body}</ul>;
}

/** Blocks as prose. `shift` adds to heading levels (a doc's `###` under a page `h2`). */
export function Markdown({ blocks, shift = 0 }: { readonly blocks: readonly Block[]; readonly shift?: number }) {
  return (
    <div className={styles.prose}>
      {blocks.map((b, i) => {
        switch (b.kind) {
          case "h": {
            const level = Math.min(6, Math.max(3, b.level + shift));
            const H = `h${level}` as "h3" | "h4" | "h5" | "h6";
            return (
              <H key={i} className={styles.h}>
                <Inline text={b.text} />
              </H>
            );
          }
          case "p":
            return (
              <p key={i}>
                <Inline text={b.text} />
              </p>
            );
          case "ul":
          case "ol":
            return <Items key={i} items={b.items} ordered={b.kind === "ol"} />;
        }
      })}
    </div>
  );
}
