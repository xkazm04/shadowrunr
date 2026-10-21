// A tone's measured record line, and the previewed tone switch: the active tone (the baseline
// being left) beside the candidate, both counted from the package's one evidence source.
// Confirm sends character.tone.set; a red-line fail warns, it never blocks.
import { useState } from "react";
import { useMutation, viewOfError } from "../../data";
import { THIN_N } from "../../kit/cellCounts";
import { int } from "../../kit/events";
import { Button } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import type { CharacterEvidence } from "./evidence";
import styles from "./Layer2.module.css";
import { thinTone, toneLabel, type ToneRecord } from "./model";
import type { ToneDoc } from "./packageDocs";

const plural = (n: number, one: string) => `${int(n)} ${n === 1 ? one : `${one}s`}`;

/** One tone's record as counts, or why there is none; never a 0 for a tone that was not run. */
function recordText(ev: CharacterEvidence, rec: ToneRecord | undefined): string {
  if (ev.listError !== undefined) return `Tournaments unavailable · ${ev.listError}`;
  if (ev.listed === undefined) return "Reading the tournaments…";
  if (ev.tid === undefined) return "Unmeasured: no tournament has run for this character";
  if (ev.tones === undefined) return ev.resultsError !== undefined ? `Results unavailable · ${ev.resultsError}` : `Reading ${ev.tid}…`;
  if (!rec) return `Unmeasured in ${ev.tid}`;
  const red = rec.redLine === 0 ? "no red-line fail" : plural(rec.redLine, "red-line fail");
  return `${plural(rec.judged, "judged run")} · ${int(rec.fail)} of ${int(rec.evaluated)} rule checks failed · ${red} in ${ev.tid}`;
}

export function ToneEvidence({ ev, tone }: { ev: CharacterEvidence; tone: string }) {
  const rec = ev.tones?.get(tone);
  return (
    <p className={styles.toneEv} data-role="characters-tone-evidence" data-measured={rec ? "true" : "false"}>
      {recordText(ev, rec)}
      {rec && thinTone(rec) && (
        <span data-role="characters-tone-thin">
          <Chip tone="caution" title={`Fewer than ${THIN_N} judged runs: a hint, not a verdict`}>
            thin
          </Chip>
        </span>
      )}
    </p>
  );
}

/**
 * The switch, previewed: the active tone (the baseline being left) beside the candidate, both
 * counted from the named tournament. Confirm sends character.tone.set; the data layer refetches
 * the package after. A candidate that failed a red line gets an alert and "Make active anyway".
 */
export function TonePreview(p: { character: string; ev: CharacterEvidence; current: ToneDoc | undefined; candidate: ToneDoc; onClose: () => void }) {
  const { ev, current, candidate } = p;
  const set = useMutation("character.tone.set");
  const [error, setError] = useState<string | undefined>(undefined);
  const name = toneLabel(candidate.title);
  const red = ev.tones?.get(candidate.id)?.redLine ?? 0;
  const confirm = () =>
    void set.run({ id: p.character, tone: candidate.id }).then((r) => {
      if (r.ok) p.onClose();
      else setError(`${viewOfError(r.error).title}: ${r.error.message}`);
    });
  const side = (label: string, t: ToneDoc) => (
    <div className={styles.kvRow}>
      <dt>
        {label} · {toneLabel(t.title)}
      </dt>
      <dd>
        <ToneEvidence ev={ev} tone={t.id} />
      </dd>
    </div>
  );
  return (
    <section className={styles.verdictBox} data-role="characters-tone-preview" aria-label={`Make ${name} the active tone`}>
      <h4>Make {name} active?</h4>
      <p className={styles.dim}>
        {ev.tid === undefined ? "No tournament has measured these tones yet: the switch is unmeasured." : `Counted from ${ev.tid}.`} Stream prep and sandbox sessions perform the active
        tone.
      </p>
      <dl className={styles.kv}>
        {current && side("Now", current)}
        {side("Candidate", candidate)}
      </dl>
      {red > 0 && (
        <p className={styles.warnInl} role="alert">
          {name} failed a red line {red === 1 ? "once" : `${int(red)} times`} in {ev.tid}.
        </p>
      )}
      <div className={styles.toolsRow}>
        <Button size="compact" variant="primary" disabled={set.pending} data-role="characters-tone-confirm" onClick={confirm}>
          {red > 0 ? "Make active anyway" : "Make active"}
        </Button>
        <Button size="compact" disabled={set.pending} data-role="characters-tone-cancel" onClick={p.onClose}>
          Cancel
        </Button>
      </div>
      {error !== undefined && <small className={styles.toneSetError}>{error}</small>}
    </section>
  );
}
