// The package's documents: the constitution (red lines first, locked) and the bible (with its
// table of contents). Both are rendered from the package markdown, never as HTML.
import { useId, useMemo } from "react";
import type { CharacterDetail } from "@shadowrunr/contract";
import { pageRoute, RouteLink } from "../../router";
import { Head, Section } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import { parseDoc, type Block } from "../../ui/doc";
import { readConstitution } from "./packageDocs";
import styles from "./Layer2.module.css";
import { Inline, Markdown } from "../../ui/Markdown";

const KICKER = "Characters · the package";

const ledeOf = (blocks: readonly Block[]): string | undefined => {
  const p = blocks.find((b) => b.kind === "p");
  return p?.kind === "p" ? p.text : undefined;
};

export function ConstitutionTab({ d }: { d: CharacterDetail }) {
  const c = useMemo(() => readConstitution(d.constitution), [d.constitution]);
  const lede = ledeOf(c.preamble);
  return (
    <>
      <Head kicker={KICKER} title="Constitution" lede={lede === undefined ? undefined : <Inline text={lede} />} />
      {c.redLines.length > 0 && (
        <Section
          title={
            <>
              Red lines {c.redLinesNote !== undefined && <small className={styles.headNote}>{c.redLinesNote}</small>}
            </>
          }
        >
          <ol className={styles.redlines} data-role="characters-redlines">
            {c.redLines.map((x) => (
              <li key={x.n} className={styles.lock}>
                <span className={styles.lk} aria-hidden="true" />
                <b>{x.n}</b>
                <div>
                  <h3>
                    <Inline text={x.short} />
                  </h3>
                  {x.text.length > 0 && (
                    <p>
                      <Inline text={x.text} />
                    </p>
                  )}
                </div>
              </li>
            ))}
          </ol>
          <p className={styles.note}>
            Red lines cannot be toggled here, in any tone or under any vote. Every judged run checks them: see <RouteLink to={pageRoute("simulator", "heatmap")}>the trials</RouteLink>.
          </p>
        </Section>
      )}
      {c.sections.map((s) => (
        <Section key={s.title} title={<Inline text={s.title} />}>
          <Markdown blocks={s.blocks} />
        </Section>
      ))}
      {c.redLines.length === 0 && c.sections.length === 0 && <EmptyState title="The constitution is empty" hint="constitution.md has no sections yet." />}
    </>
  );
}

export function BibleTab({ d }: { d: CharacterDetail }) {
  const prefix = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const doc = useMemo(() => (d.bible === undefined ? undefined : parseDoc(d.bible)), [d.bible]);
  if (doc === undefined) {
    return (
      <>
        <Head kicker={KICKER} title="Bible" />
        <EmptyState title="No bible yet" hint="bible.md is optional; it holds lore, wants, the long arc and running jokes." />
      </>
    );
  }
  const lede = ledeOf(doc.preamble);
  const anchor = (i: number) => `${prefix}-b${i}`;
  return (
    <>
      <Head kicker={KICKER} title="Bible" lede={lede === undefined ? "Lore, want, the long arc and running jokes." : <Inline text={lede} />} />
      <div className={styles.bible}>
        <nav className={styles.toc} aria-label="Bible sections">
          {doc.sections.map((s, i) => (
            <button key={s.title} type="button" onClick={() => document.getElementById(anchor(i))?.scrollIntoView({ behavior: "smooth", block: "start" })}>
              {s.title.replace(/\s*\(in-world:.*\)\s*$/, "")}
            </button>
          ))}
        </nav>
        <div className={styles.bibleBody}>
          {doc.sections.map((s, i) => (
            <section key={s.title} id={anchor(i)} className={styles.bibleSec}>
              <h2>
                <Inline text={s.title} />
              </h2>
              <Markdown blocks={s.blocks} />
            </section>
          ))}
        </div>
      </div>
    </>
  );
}
