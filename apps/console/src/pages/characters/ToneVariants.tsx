// The variants panel: the core model drafts up to five candidate lines for one situation in the
// tone (tone.variants), from the tone, the directions, a few bank lines and, when given, a line
// to improve and the owner's note. Nothing is written until the owner approves a candidate, as
// written or edited, into the bank (tone.lines.add).
import { useState } from "react";
import type { LineSituation } from "@shadowrunr/contract";
import { useMutation, viewOfError, type DataError } from "../../data";
import { Button } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import { StatePanel } from "../../ui/States";
import styles from "./Layer2.module.css";
import lab from "./ToneLab.module.css";
import type { LabProps, Seed } from "./toneTypes";
import { SITUATION_NAME, SITUATION_ORDER } from "./toneNames";

/** The core model's route is 30 s; the call also loads the package and runs the gate. */
const VARIANTS_TIMEOUT_MS = 90_000;
const COUNTS = [1, 2, 3, 4, 5] as const;

interface Draft {
  readonly text: string;
  readonly original: string;
  /** The id it was added under. */
  readonly added?: string | undefined;
  readonly error?: string | undefined;
}

export function ToneVariants(props: LabProps & { seed: Seed | undefined }) {
  const generate = useMutation("tone.variants", { timeoutMs: VARIANTS_TIMEOUT_MS });
  const add = useMutation("tone.lines.add");
  // The panel is keyed by its seed, so a new seed starts it afresh from these.
  const seed = props.seed;
  const [situation, setSituation] = useState<LineSituation>(seed?.situation ?? "roast_request");
  const [n, setN] = useState(3);
  const [line, setLine] = useState(seed?.line ?? "");
  const [note, setNote] = useState(seed?.note ?? "");
  const source = seed?.source;
  const [drafts, setDrafts] = useState<readonly Draft[] | undefined>(undefined);
  const [gated, setGated] = useState(0);
  const [failure, setFailure] = useState<DataError | undefined>(undefined);


  const run = async () => {
    setFailure(undefined);
    const r = await generate.run({
      character: props.character,
      tone: props.tone,
      situation,
      n,
      ...(line.trim() === "" ? {} : { line: line.trim() }),
      ...(note.trim() === "" ? {} : { note: note.trim() }),
    });
    if (!r.ok) {
      setFailure(r.error);
      return;
    }
    setDrafts(r.value.candidates.map((c) => ({ text: c, original: c })));
    setGated(r.value.gated ?? 0);
  };

  const edit = (i: number, patch: Partial<Draft>) => setDrafts((xs) => xs?.map((x, j) => (j === i ? { ...x, ...patch } : x)));

  const approve = async (i: number) => {
    const draft = drafts?.[i];
    if (!draft) return;
    edit(i, { error: undefined });
    const r = await add.run({ character: props.character, tone: props.tone, text: draft.text, tags: [situation], status: "approved" });
    if (!r.ok) {
      edit(i, { error: `${viewOfError(r.error).title}: ${r.error.message}` });
      return;
    }
    edit(i, { added: r.value.line.id });
    props.onWrote(r.value.file);
  };

  return (
    <section className={lab.panel} data-role="tone-lab-variants" aria-labelledby="tone-lab-variants">
      <h2 className={styles.secHead} id="tone-lab-variants">
        Variants
      </h2>
      <div className={styles.toolsRow}>
        <label className={styles.fld}>
          Situation
          <select value={situation} onChange={(e) => setSituation(e.target.value as LineSituation)} data-role="tone-lab-situation">
            {SITUATION_ORDER.map((s) => (
              <option key={s} value={s}>
                {SITUATION_NAME[s]}
              </option>
            ))}
          </select>
        </label>
        <label className={styles.fld}>
          Lines
          <select value={n} onChange={(e) => setN(Number(e.target.value))}>
            {COUNTS.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label className={lab.label} htmlFor="tone-lab-line">
        A line to improve, optional{source !== undefined && <span className={lab.source}> · from {source}</span>}
      </label>
      <textarea id="tone-lab-line" className={lab.field} value={line} maxLength={2000} onChange={(e) => setLine(e.target.value)} data-role="tone-lab-line" />
      <label className={lab.label} htmlFor="tone-lab-note">
        Your note, optional
      </label>
      <textarea id="tone-lab-note" className={lab.field} value={note} maxLength={2000} placeholder="What should change: meaner, name the vote, shorter…" onChange={(e) => setNote(e.target.value)} data-role="tone-lab-note" />
      <div className={lab.actions}>
        <Button variant="ink" disabled={generate.pending} onClick={() => void run()} data-role="tone-lab-generate">
          {generate.pending ? "Writing…" : `Draft ${n === 1 ? "a line" : `${n} lines`}`}
        </Button>
        <span className={styles.dim}>The core model writes them; nothing is saved until you approve one.</span>
      </div>
      {failure && <StatePanel state={viewOfError(failure).state} title={`No variants · ${viewOfError(failure).title}`} hint={viewOfError(failure).hint} detail={`${failure.code}: ${failure.message}`} />}
      {drafts && drafts.length === 0 && (
        <StatePanel state="empty" title="The model gave no new line" hint={gated > 0 ? `${gated} would have tripped the output gate and were dropped. Try again, or change the note.` : "Every line it wrote is already in the bank. Try again, or change the note."} />
      )}
      {drafts && drafts.length > 0 && (
        <ol className={lab.list} data-role="tone-lab-candidates">
          {drafts.map((c, i) => {
            const edited = c.text.trim() !== c.original;
            return (
              <li key={i} className={lab.item} data-added={c.added !== undefined}>
                <textarea className={lab.field} aria-label={`Candidate ${i + 1}`} value={c.text} maxLength={400} disabled={c.added !== undefined} onChange={(e) => edit(i, { text: e.target.value })} />
                <div className={lab.actions}>
                  {c.added !== undefined ? (
                    <Chip tone="ok">Approved as {c.added}</Chip>
                  ) : (
                    <Button size="compact" disabled={add.pending || c.text.trim() === ""} onClick={() => void approve(i)}>
                      {edited ? "Approve as edited" : "Approve"}
                    </Button>
                  )}
                  {c.error !== undefined && <span className={lab.err}>{c.error}</span>}
                </div>
              </li>
            );
          })}
        </ol>
      )}
      {drafts && drafts.length > 0 && gated > 0 && <p className={styles.note}>{gated} more would have tripped the output gate and were dropped.</p>}
    </section>
  );
}
