// The Tone Lab's side column. The inbox (tone.inbox): Review lines marked Great roast or Landed
// are bank candidates (add one as an approved line of its session's tone, or draft variants of
// it); lines marked Flat or Too mean are direction suggestions (append the note to the
// directions draft, or draft variants with it). The directions (tone.directions.get / .set):
// directions.md as a plain text area, saved whole.
import { useState } from "react";
import type { ToneReviewItem } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { useMutation, useRpc, viewOfError, type DataError } from "../../data";
import { Button } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import { StatePanel } from "../../ui/States";
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
  const q = useRpc("tone.directions.get", { character: props.character });
  const save = useMutation("tone.directions.set");
  const [draft, setDraft] = useState<string | undefined>(undefined);
  const [failure, setFailure] = useState<DataError | undefined>(undefined);
  const [saved, setSaved] = useState(false);
  const text = draft ?? q.data?.text ?? "";
  const dirty = draft !== undefined && draft !== q.data?.text;

  // An appended suggestion lands in the draft once per request (state adjusted in render).
  const [appended, setAppended] = useState<Append | undefined>(undefined);
  if (props.append && props.append !== appended) {
    setAppended(props.append);
    const base = text.replace(/\s+$/, "");
    setDraft(`${base}${base === "" ? "" : "\n"}- ${props.append.text}\n`);
    setSaved(false);
  }

  const run = async () => {
    setFailure(undefined);
    const r = await save.run({ character: props.character, text });
    if (!r.ok) return setFailure(r.error);
    setDraft(undefined);
    setSaved(true);
    props.onWrote(r.value.file);
    q.reload();
  };

  return (
    <section className={lab.panel} data-role="tone-lab-directions" aria-labelledby="tone-lab-directions">
      <h2 className={styles.secHead} id="tone-lab-directions">
        Directions
      </h2>
      <p className={styles.note}>The do and don&apos;t list, read with the tone on every stream (directions.md).</p>
      <QueryView query={q}>
        {() => (
          <>
            <textarea
              className={`${lab.field} ${lab.tall}`}
              aria-label="Directions"
              value={text}
              maxLength={20000}
              onChange={(e) => {
                setDraft(e.target.value);
                setSaved(false);
              }}
              data-role="tone-lab-directions-text"
            />
            <div className={lab.actions}>
              <Button variant="ink" disabled={save.pending || !dirty} onClick={() => void run()} data-role="tone-lab-directions-save">
                {save.pending ? "Saving…" : "Save directions"}
              </Button>
              {dirty && (
                <Button size="compact" onClick={() => setDraft(undefined)}>
                  Discard changes
                </Button>
              )}
              {saved && !dirty && <span className={lab.ok}>Saved</span>}
            </div>
            {failure && <StatePanel state={viewOfError(failure).state} title={`Not saved · ${viewOfError(failure).title}`} hint={viewOfError(failure).hint} detail={`${failure.code}: ${failure.message}`} />}
          </>
        )}
      </QueryView>
    </section>
  );
}
