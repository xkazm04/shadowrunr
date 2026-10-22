// The player's tag line, its side column (where, with the owner's Direct actions; the judge on
// this line, every instance across the session, the way to the timeline) and its done state.
import type { ReviewSession } from "@shadowrunr/contract";
import { Button } from "../../ui/Button";
import { Chip, ChipRow } from "../../ui/Chip";
import type { Derived, SayLine } from "./derive";
import { DirectActions } from "./Direct";
import { segmentAt } from "./derive";
import type { Feedback } from "./hooks";
import { KIND_INK } from "./ink";
import { JudgeOn } from "./Moment";
import { EDGE_NAME, FEEDBACK, fmtClock, KIND_NAME, SEGMENT_NAME, TARGET_NAME, type ShownKind } from "./names";
import fb from "./FeedbackPanel.module.css";
import styles from "./Player.module.css";
import kit from "./tabkit.module.css";
import { TickStrip } from "./tabkit";

/** The line's kind, whom it is aimed at, how sharp, and the tagger's reason. */
export function TagLine({ say }: { say: SayLine }) {
  const t = say.tag;
  return (
    <p className={styles.tag}>
      <Chip>
        <span className={styles.swatch} style={{ background: KIND_INK[say.kind] }} /> {KIND_NAME[say.kind]}
      </Chip>
      <span className={styles.aim}>
        {t
          ? `aimed at ${TARGET_NAME[t.target]} · ${EDGE_NAME[t.edge]}${t.why ? ` · “${t.why}”` : ""}`
          : say.kind === "stall"
            ? "a stall line, from the engine record"
            : "not tagged yet"}
      </span>
    </p>
  );
}

export function PlayerSide(props: {
  rs: ReviewSession;
  d: Derived;
  list: readonly SayLine[];
  cur: number;
  marked: (line: number) => boolean;
  openLine: (line: number) => void;
}) {
  const x = props.list[props.cur];
  if (!x) return null;
  const seg = segmentAt(props.rs, x.t);
  const ticks = props.list.map((s, i) => ({ t: s.t, ...(i === props.cur ? { tone: "on" as const } : props.marked(s.line) ? { tone: "ok" as const } : {}) }));
  return (
    <aside className={styles.side} data-role="review-player-side">
      <div>
        <h3 className={kit.sub}>Where</h3>
        <p className={styles.where}>
          {fmtClock(x.t)}
          {seg ? ` · ${SEGMENT_NAME[seg] ?? "Segment"}` : ""} · line {x.line}
        </p>
        <DirectActions rs={props.rs} say={x} />
      </div>
      <div>
        <h3 className={kit.sub}>Judge</h3>
        <JudgeOn d={props.d} line={x.line} />
      </div>
      <div>
        <h3 className={kit.sub}>This type in the session</h3>
        <TickStrip ticks={ticks} lengthS={props.d.lengthS} label={`${props.list.length} lines of this type; the current one is highlighted`} axis />
      </div>
      <Button onClick={() => props.openLine(x.line)}>Open on the timeline</Button>
    </aside>
  );
}

/** Marks the daemon did not take, named by line, with one Retry that resends each as it was. */
export function FailedMarks({ feedback }: { feedback: Feedback }) {
  const n = feedback.failed.length;
  if (n === 0) return null;
  const lines = feedback.failed.map((f) => f.line);
  return (
    <p className={styles.tag} role="alert" data-role="review-failed-marks">
      <span className={fb.err}>
        {n} {n === 1 ? "mark" : "marks"} not saved · {n === 1 ? "line" : "lines"} {lines.join(", ")}
      </span>
      <Button onClick={() => lines.forEach((l) => feedback.retry(l))}>Retry</Button>
    </p>
  );
}

export function PlayerDone(props: { kind: ShownKind; list: readonly SayLine[]; feedback: Feedback; onPick: () => void; onAgain: () => void }) {
  const totals = new Map<string, number>();
  for (const s of props.list) for (const t of props.feedback.byLine.get(s.line)?.tags ?? []) totals.set(t, (totals.get(t) ?? 0) + 1);
  const rows = FEEDBACK.filter(([id]) => totals.has(id)).sort((a, b) => (totals.get(b[0]) ?? 0) - (totals.get(a[0]) ?? 0));
  return (
    <section className={styles.done} data-role="review-player-done">
      <div className={styles.pos}>
        {KIND_NAME[props.kind]} · {props.list.length} lines
      </div>
      <h2 className={styles.doneTitle}>{props.list.length === 0 ? "No line of this type in this session" : "Every line has your mark"}</h2>
      {props.list.length > 0 &&
        (rows.length === 0 ? (
          <p className={kit.note}>Marked with notes only.</p>
        ) : (
          <ChipRow>
            {rows.map(([id, name]) => (
              <Chip key={id}>
                {name} <b className={styles.total}>{totals.get(id)}</b>
              </Chip>
            ))}
          </ChipRow>
        ))}
      <div className={styles.actions}>
        <Button onClick={props.onPick}>Pick another type</Button>
        {props.list.length > 0 && (
          <Button variant="ink" onClick={props.onAgain}>
            Step through all {props.list.length} again
          </Button>
        )}
      </div>
    </section>
  );
}
