// The Tone Lab's side column. The inbox (tone.inbox): Review lines marked Great roast or Landed
// are bank candidates (add one as an approved line of its session's tone, or draft variants of
// it); lines marked Flat or Too mean are direction suggestions (append the note to the
// directions draft, or draft variants with it). The directions (tone.directions.get / .set):
// directions.md in the shared directions editor, saved whole.
import { useState } from "react";
import type { ToneReviewItem } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { BehaviourDirectionsEditor } from "../../kit/Directions";
import { useMutation, useRpc, viewOfError } from "../../data";
import { Button } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import { toneLabel } from "./model";
import styles from "./Layer2.module.css";
import lab from "./ToneLab.module.css";
import { FEEDBACK_NAME, reviewRef, SITUATION_NAME } from "./toneNames";
import { nextKey, type Append, type LabProps, type Seed } from "./toneTypes";

/** Items shown per inbox list; the rest wait until these are dealt with. */
const SHOWN = 12;

export function ToneInbox(props: LabProps & { tones: readonly string[]; onImprove: (s: Seed) => void; onSuggest: (text: string) => void }) {
  const inbox = useRpc("tone.inbox", { character: props.character });
  return (
    <section className={lab.panel} data-role="tone-lab-inbox" aria-labelledby="tone-lab-inbox">
      <h2 className={styles.secHead} id="tone-lab-inbox">
        From Review
      </h2>
      <QueryView
        query={inbox}
        isEmpty={(ib) => ib.candidates.length === 0 && ib.suggestions.length === 0}
        emptyTitle="Nothing from Review yet"
        emptyHint="Mark lines in a Review session: Great roast or Landed make bank candidates, Flat or Too mean make direction suggestions."
      >
        {(ib) => (
          <>
            <h3 className={lab.sub}>Bank candidates · {ib.candidates.length}</h3>
            {ib.candidates.length === 0 ? (
              <p className={styles.note}>No marked line is waiting: each one you add leaves this list.</p>
            ) : (
              <ul className={lab.list} data-role="tone-lab-candidates-in">
                {ib.candidates.slice(0, SHOWN).map((i) => (
                  <Candidate key={`${i.session_id}#${i.line}`} i={i} {...props} reload={inbox.reload} />
                ))}
              </ul>
            )}
            <h3 className={lab.sub}>Direction suggestions · {ib.suggestions.length}</h3>
            {ib.suggestions.length === 0 ? (
              <p className={styles.note}>No line marked Flat or Too mean.</p>
            ) : (
              <ul className={lab.list} data-role="tone-lab-suggestions">
                {ib.suggestions.slice(0, SHOWN).map((i) => (
                  <li key={`${i.session_id}#${i.line}`} className={lab.item}>
                    <Said i={i} />
                    {i.note !== undefined && <p className={lab.noteText}>“{i.note}”</p>}
                    <div className={lab.actions}>
                      <Button size="compact" onClick={() => props.onSuggest(i.note ?? `Not like this: “${i.text}”`)}>
                        Add to directions
                      </Button>
                      <Button size="compact" onClick={() => props.onImprove(seedOf(i))}>
                        Variants of it
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </QueryView>
    </section>
  );
}

const seedOf = (i: ToneReviewItem): Seed => ({
  key: nextKey(),
  line: i.text,
  ...(i.note === undefined ? {} : { note: i.note }),
  ...(i.situation === undefined ? {} : { situation: i.situation }),
  source: reviewRef(i.session_id, i.line),
});

function Said({ i }: { i: ToneReviewItem }) {
  return (
    <>
      <p className={lab.text}>{i.text}</p>
      <div className={lab.meta}>
        {i.tags.map((t) => (
          <Chip key={t}>{FEEDBACK_NAME[t]}</Chip>
        ))}
        {i.situation !== undefined && <Chip tone="neutral">{SITUATION_NAME[i.situation]}</Chip>}
        <span className={styles.dim}>
          {reviewRef(i.session_id, i.line)} · {toneLabel(i.tone)}
        </span>
      </div>
    </>
  );
}

function Candidate({ i, reload, ...props }: LabProps & { i: ToneReviewItem; tones: readonly string[]; reload: () => void; onImprove: (s: Seed) => void }) {
  const add = useMutation("tone.lines.add");
  const [error, setError] = useState<string | undefined>(undefined);
  // The line joins the bank of the tone it was spoken in, when the package still has it.
  const tone = props.tones.includes(i.tone) ? i.tone : props.tone;
  const approve = async () => {
    setError(undefined);
    const r = await add.run({ character: props.character, tone, text: i.text, tags: [i.situation ?? "general"], status: "approved" });
    if (!r.ok) return setError(`${viewOfError(r.error).title}: ${r.error.message}`);
    props.onWrote(r.value.file);
    reload();
  };
  return (
    <li className={lab.item}>
      <Said i={i} />
      <div className={lab.actions}>
        <Button size="compact" variant="ink" disabled={add.pending} onClick={() => void approve()}>
          Add to the {toneLabel(tone)} bank
        </Button>
        <Button size="compact" onClick={() => props.onImprove(seedOf(i))}>
          Variants of it
        </Button>
        {error !== undefined && <span className={lab.err}>{error}</span>}
      </div>
    </li>
  );
}

export function ToneDirections(props: LabProps & { append: Append | undefined }) {
  // A suggestion joins the do and don't list as one more item.
  const append = props.append && { key: props.append.key, text: `- ${props.append.text}` };
  return (
    <section className={lab.panel} data-role="tone-lab-directions" aria-labelledby="tone-lab-directions">
      <h2 className={styles.secHead} id="tone-lab-directions">
        Directions
      </h2>
      <p className={styles.note}>The do and don&apos;t list, read with the tone on every stream (directions.md).</p>
      <BehaviourDirectionsEditor character={props.character} role="tone-lab-directions" append={append} onSaved={props.onWrote} />
    </section>
  );
}
