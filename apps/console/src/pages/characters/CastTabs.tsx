// The package's voices: tones (the tournament axis, the active one as the hero; any other can be
// made active, an operator choice kept in <data> by character.tone.set) and foils (the SEG-01
// guests, drawn by stage-kit's one FoilArt). Each tone carries its measured record from the
// package's one evidence source (useCharacterEvidence, the tournament Situations reads), and
// Make active previews the switch, current beside candidate, before it sends: the active tone is
// what stream prep and sandbox sessions perform. A red-line fail warns; it never blocks.
import { FoilArt, hasFoilArt } from "@shadowrunr/stage-kit/ui";
import { useMemo, useState } from "react";
import type { CharacterDetail } from "@shadowrunr/contract";
import { pageRoute, RouteLink } from "../../router";
import { Button } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import { Head, Section } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import { readFoil, readTone, type ToneDoc } from "./packageDocs";
import styles from "./Layer2.module.css";
import { Inline } from "../../ui/Markdown";
import { useCharacterEvidence, type CharacterEvidence } from "./evidence";
import { foilName, toneLabel } from "./model";
import { ToneEvidence, TonePreview } from "./TonePreview";

const KICKER = "Characters · the package";

/** A tone's labelled lines: the current shape/segment/openings set, or an older tone's ratio. */
function toneRows(t: ToneDoc): readonly (readonly [string, string | undefined])[] {
  return [
    ["Shape", t.shape],
    ["Segment", t.segment],
    ["Ratio", t.ratio],
    ["Edge", t.edge],
    ["Openings", t.openings],
  ];
}

function Kv({ rows }: { rows: readonly (readonly [string, string | undefined])[] }) {
  const shown = rows.filter((r): r is readonly [string, string] => r[1] !== undefined && r[1].length > 0);
  if (shown.length === 0) return null;
  return (
    <dl className={styles.kv}>
      {shown.map(([k, v]) => (
        <div key={k} className={styles.kvRow}>
          <dt>{k}</dt>
          <dd>
            <Inline text={v} />
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function TonesTab({ d }: { d: CharacterDetail }) {
  const tones = useMemo(() => d.tones.map((t) => readTone(t.id, t.text)), [d.tones]);
  const ev = useCharacterEvidence(d);
  const [candidate, setCandidate] = useState<string | undefined>(undefined);
  const hero = tones.find((t) => t.id === d.active_tone) ?? tones.find((t) => t.id === d.default_tone);
  const others = tones.filter((t) => t !== hero);
  return (
    <>
      <Head kicker={KICKER} title="Tones" lede="One file per tone variant. The tone is the tournament axis: the same character, performed several ways, judged blind." />
      {tones.length === 0 && <EmptyState title="No tones yet" hint="Add tones/<id>.md to the package." />}
      {hero && <ToneHero t={hero} isDefault={hero.id === d.default_tone} defaultTone={d.default_tone} ev={ev} />}
      {others.length > 0 && (
        <Section title={hero ? `The other ${others.length}` : "Tones"}>
          <div className={styles.toneGrid}>
            {others.map((t) => (
              <article key={t.id} className={styles.toneCard} data-role="characters-tone-card">
                <header>
                  <h3>{toneLabel(t.title)}</h3>
                  {t.baseline && <Chip tone="ok">baseline</Chip>}
                  {candidate !== t.id && (
                    <Button size="compact" className={styles.toneSet} data-role="characters-tone-set" onClick={() => setCandidate(t.id)}>
                      Make active
                    </Button>
                  )}
                </header>
                <ToneEvidence ev={ev} tone={t.id} />
                {candidate === t.id && <TonePreview character={d.id} ev={ev} current={hero} candidate={t} onClose={() => setCandidate(undefined)} />}
                {t.desc !== undefined && (
                  <p>
                    <Inline text={t.desc} />
                  </p>
                )}
                <Kv rows={toneRows(t)} />
                {t.examples[0] !== undefined && (
                  <blockquote className={styles.voice}>
                    “<Inline text={t.examples[0]} />”
                  </blockquote>
                )}
              </article>
            ))}
          </div>
        </Section>
      )}
    </>
  );
}

function ToneHero({ t, isDefault, defaultTone, ev }: { t: ToneDoc; isDefault: boolean; defaultTone: string; ev: CharacterEvidence }) {
  return (
    <section className={styles.toneHero} data-role="characters-tone-hero">
      <div>
        <span className={styles.actv}>Active tone</span>
        <h2>{toneLabel(t.title)}</h2>
        {t.desc !== undefined && (
          <p className={styles.desc}>
            <Inline text={t.desc} />
          </p>
        )}
        <Kv rows={toneRows(t)} />
        <ToneEvidence ev={ev} tone={t.id} />
        <p className={styles.heroNote}>
          {isDefault ? "The package default" : `The operator's choice (the package default is ${toneLabel(defaultTone)})`}
          {t.baseline ? " and the measured baseline" : ""}. Each tone&apos;s record is counted from its cells, which are in <RouteLink to={pageRoute("simulator", "heatmap")}>the trials</RouteLink>.
        </p>
      </div>
      <div>
        {t.examples.length > 0 && (
          <>
            <h3>Example lines</h3>
            <ol className={styles.quotes}>
              {t.examples.map((x, i) => (
                <li key={i} className={styles.voice}>
                  “<Inline text={x} />”
                </li>
              ))}
            </ol>
          </>
        )}
        {t.moves.length > 0 && (
          <>
            <h3>Signature moves</h3>
            <ul className={styles.plain}>
              {t.moves.map((m, i) => (
                <li key={i}>
                  <Inline text={m} />
                </li>
              ))}
            </ul>
          </>
        )}
        {t.bet !== undefined && (
          <>
            <h3>The bet</h3>
            <p>
              <Inline text={t.bet} />
            </p>
          </>
        )}
      </div>
    </section>
  );
}

export function FoilsTab({ d }: { d: CharacterDetail }) {
  const foils = useMemo(() => d.foils.map((f) => ({ f, name: foilName(f), doc: readFoil(f.text) })), [d.foils]);
  return (
    <>
      <Head kicker={KICKER} title="Foils" lede="Guest characters for SEG-01. Original parody archetypes, never real people. A foil speaks at most one sentence per turn; the character always has the last word." />
      {foils.length === 0 ? (
        <EmptyState title="No foils yet" hint="Add foils/<id>.md and list the id in character.toml to make it summonable." />
      ) : (
        <div className={styles.foilGrid}>
          {foils.map(({ f, name, doc }) => (
            <article key={f.id} className={styles.foilCard} data-role="characters-foil-card">
              <div className={styles.fa}>{hasFoilArt(f.id) ? <FoilArt id={f.id} /> : <span className={styles.noArt}>{name.replace(/^The /, "").charAt(0)}</span>}</div>
              <div>
                <h3>
                  {name} {!f.declared && <Chip tone="caution" title="Not listed in character.toml, so it cannot be summoned">draft</Chip>}
                </h3>
                {doc.desc !== undefined && (
                  <p>
                    <Inline text={doc.desc} />
                  </p>
                )}
                <Kv rows={[["Function", doc.fn], ["Signature bits", doc.bits], ["Red lines", doc.redLines]]} />
              </div>
            </article>
          ))}
        </div>
      )}
      {foils.length > 0 && (
        <p className={styles.note}>
          The art is stylised, drawn for this console. Foil ids:{" "}
          {foils.map(({ f }) => (
            <code key={f.id}>{f.id} </code>
          ))}
        </p>
      )}
    </>
  );
}
