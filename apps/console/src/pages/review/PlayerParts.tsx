// The player's tag line, its side panel (where, the judge on this line, every instance across the
// session, the way to the timeline) and its done state.
import type { ReviewSession } from "@shadowrunr/contract";
import type { Derived, SayLine } from "./derive";
import { segmentAt } from "./derive";
import type { Feedback } from "./hooks";
import { KIND_INK } from "./ink";
import { dimName, EDGE_NAME, FEEDBACK, fmtClock, KIND_NAME, ruleName, SEGMENT_NAME, TARGET_NAME, type ShownKind } from "./names";
import styles from "./Player.module.css";
import kit from "./tabkit.module.css";
import { TickStrip } from "./tabkit";

/** The line's kind, whom it is aimed at, how sharp, and the tagger's reason. */
export function TagLine({ say }: { say: SayLine }) {
  const t = say.tag;
  return (
    <p className={styles.tag}>
      <span className={styles.chip}>
        <span className={styles.swatch} style={{ background: KIND_INK[say.kind] }} />
        {KIND_NAME[say.kind]}
      </span>
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

/** What the judge said about one line: rules it failed there and its sampled quality facts. */
export function JudgeOnLine({ d, line }: { d: Derived; line: number }) {
  const fails = d.failLines.get(line) ?? [];
  const facts = d.facts.get(line) ?? [];
  if (fails.length === 0 && facts.length === 0) return <p className={kit.empty}>The judge did not sample this line.</p>;
  return (
    <ul className={styles.facts}>
      {fails.map((r) => (
        <li key={`f${r}`} className={styles.bad}>
          Failed: {ruleName(r)}
        </li>
      ))}
      {facts.map(([dim, v]) => (
        <li key={`q${dim}`} className={v ? styles.good : styles.bad}>
          {v ? "✓" : "✗"} {dimName(dim)}
        </li>
      ))}
    </ul>
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
    <aside className={`${kit.panel} ${kit.stack}`} data-role="review-player-side">
      <div>
        <div className={kit.kicker}>Where</div>
        <p className={styles.where}>
          {fmtClock(x.t)}
          {seg ? ` · ${SEGMENT_NAME[seg] ?? "Segment"}` : ""} · line {x.line}
        </p>
      </div>
      <div>
        <div className={kit.kicker}>Judge</div>
        <JudgeOnLine d={props.d} line={x.line} />
      </div>
      <div>
        <div className={kit.kicker}>This type in the session</div>
        <TickStrip ticks={ticks} lengthS={props.d.lengthS} label={`${props.list.length} lines of this type; the current one is highlighted`} axis />
      </div>
      <button type="button" className={kit.btn} onClick={() => props.openLine(x.line)}>
        Open on the timeline
      </button>
    </aside>
  );
}

export function PlayerDone(props: { kind: ShownKind; list: readonly SayLine[]; feedback: Feedback; onPick: () => void; onAgain: () => void }) {
  const totals = new Map<string, number>();
  for (const s of props.list) for (const t of props.feedback.byLine.get(s.line)?.tags ?? []) totals.set(t, (totals.get(t) ?? 0) + 1);
  const rows = FEEDBACK.filter(([id]) => totals.has(id)).sort((a, b) => (totals.get(b[0]) ?? 0) - (totals.get(a[0]) ?? 0));
  return (
    <section className={`${kit.panel} ${kit.stack}`} data-role="review-player-done">
      <div className={kit.kicker}>
        {KIND_NAME[props.kind]} · {props.list.length} lines
      </div>
      <h2 className={styles.doneTitle}>{props.list.length === 0 ? "No line of this type in this session" : "Every line has your mark"}</h2>
      {props.list.length > 0 && (
        <ul className={styles.totals}>
          {rows.length === 0 ? (
            <li className={kit.empty}>Marked with notes only.</li>
          ) : (
            rows.map(([id, name]) => (
              <li key={id}>
                {name} <b>{totals.get(id)}</b>
              </li>
            ))
          )}
        </ul>
      )}
      <div className={styles.actions}>
        <button type="button" className={kit.btn} onClick={props.onPick}>
          Pick another type
        </button>
        {props.list.length > 0 && (
          <button type="button" className={kit.btn} onClick={props.onAgain}>
            Step through all {props.list.length} again
          </button>
        )}
      </div>
    </section>
  );
}
