// The timeline's right drawer, over the page with its own scroll. Two states: a row's lines
// (time, kind, his words, ✓ when marked), and one moment: the script around the line, its tag,
// the judge on it and your feedback. Focus moves into it on open; the timeline returns it.
import { useEffect, useId, useRef } from "react";
import type { ReviewSession } from "@shadowrunr/contract";
import { visibleSays, type Bucket } from "./buckets";
import { segmentAt, tsOf, type Derived, type SayLine } from "./derive";
import { FeedbackPanel } from "./FeedbackPanel";
import type { Feedback } from "./hooks";
import { dimName, EDGE_NAME, fmtClock, KIND_INK, KIND_NAME, ruleName, SEGMENT_NAME, TARGET_NAME, type ShownKind } from "./names";
import { Script } from "./Script";
import styles from "./Drawer.module.css";

export interface DrawerProps {
  readonly rs: ReviewSession;
  readonly d: Derived;
  readonly feedback: Feedback;
  /** The selected row, when the drawer came from one. */
  readonly bucket: Bucket | undefined;
  /** The open moment's line; without one the drawer lists the row. */
  readonly line: number | undefined;
  readonly shown: ReadonlySet<ShownKind>;
  readonly onOpenLine: (line: number) => void;
  readonly onBack: () => void;
  readonly onClose: () => void;
}

const segName = (rs: ReviewSession, t: number): string => SEGMENT_NAME[segmentAt(rs, t) ?? ""] ?? "";

export function Drawer(props: DrawerProps) {
  const ref = useRef<HTMLElement>(null);
  const titleId = useId();
  const view = props.line !== undefined ? `m${props.line}` : `r${props.bucket?.i ?? ""}`;
  // Focus moves into the drawer whenever it opens or changes what it shows.
  useEffect(() => {
    ref.current?.focus({ preventScroll: true });
  }, [view]);
  return (
    <aside ref={ref} className={styles.drawer} role="dialog" aria-modal="false" aria-labelledby={titleId} tabIndex={-1} data-role="review-drawer">
      {props.line !== undefined ? <Moment {...props} line={props.line} titleId={titleId} /> : props.bucket ? <RowList {...props} bucket={props.bucket} titleId={titleId} /> : null}
    </aside>
  );
}

function Close({ onClose }: { onClose: () => void }) {
  return (
    <button type="button" className={styles.close} aria-label="Close details" onClick={onClose} data-role="review-drawer-close">
      ×
    </button>
  );
}

function RowList(props: DrawerProps & { bucket: Bucket; titleId: string }) {
  const b = props.bucket;
  const vis = visibleSays(b, props.shown);
  const nv = vis.filter((s) => props.feedback.byLine.has(s.line)).length;
  const seg = segName(props.rs, (b.from + b.to) / 2);
  return (
    <>
      <div className={styles.dhead}>
        <div>
          <div className={styles.kicker}>
            {fmtClock(b.from)} to {fmtClock(b.to)}
            {seg ? ` · ${seg}` : ""}
          </div>
          <h3 id={props.titleId} className={styles.title}>
            {vis.length} of {b.says.length} lines shown · {nv} marked
          </h3>
        </div>
        <Close onClose={props.onClose} />
      </div>
      {vis.length === 0 ? (
        <p className={styles.empty}>No lines of the shown types here.</p>
      ) : (
        <ul className={styles.mlist} data-role="review-drawer-lines">
          {vis.map((s) => (
            <li key={s.line}>
              <button type="button" className={styles.mbtn} onClick={() => props.onOpenLine(s.line)} data-line={s.line}>
                <span className={styles.mt}>{fmtClock(s.t)}</span>
                <span className={styles.sw} style={{ background: KIND_INK[s.kind] }} title={KIND_NAME[s.kind]} />
                <span className={styles.tx}>{s.text}</span>
                <span className={styles.check} aria-label={props.feedback.byLine.has(s.line) ? "marked" : undefined}>
                  {props.feedback.byLine.has(s.line) ? "✓" : ""}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function Moment(props: DrawerProps & { line: number; titleId: string }) {
  const { rs, d, line, bucket: b } = props;
  const e = rs.events[line - 1];
  if (!e) return null;
  const t = tsOf(e);
  const seg = segName(rs, t);
  const say = d.sayByLine.get(line);
  return (
    <>
      <div className={styles.dhead}>
        <div>
          <div className={styles.kicker} data-role="review-moment-kicker">
            {fmtClock(t)}
            {seg ? ` · ${seg}` : ""} · line {line}
          </div>
          <h3 id={props.titleId} className={styles.title}>
            The moment
          </h3>
        </div>
        <div className={styles.actions}>
          {b && (
            <button type="button" className={styles.btn} onClick={props.onBack} data-role="review-drawer-back">
              All lines in {fmtClock(b.from)}–{fmtClock(b.to)}
            </button>
          )}
          <Close onClose={props.onClose} />
        </div>
      </div>
      <div className={styles.stack}>
        <Script rs={rs} d={d} line={line} />
        {say ? <TagMeta say={say} /> : <p className={styles.empty}>This is not one of Malakor’s lines; the timeline marks only his.</p>}
        <section aria-label="The judge on this line">
          <div className={styles.kicker}>Judge</div>
          <JudgeOn d={d} line={line} />
        </section>
        {say && <FeedbackPanel line={line} feedback={props.feedback} />}
      </div>
    </>
  );
}

function TagMeta({ say }: { say: SayLine }) {
  const tag = say.tag;
  return (
    <div className={styles.meta} data-role="review-moment-tag">
      <span className={styles.chip}>
        <span className={styles.sw} style={{ background: KIND_INK[say.kind] }} />
        {KIND_NAME[say.kind]}
      </span>
      {say.kind === "stall" ? (
        <span className={styles.mute}>derived from the engine record</span>
      ) : tag ? (
        <span className={styles.mute}>
          aimed at {TARGET_NAME[tag.target]} · {EDGE_NAME[tag.edge]}
          {tag.why ? ` · “${tag.why}”` : ""}
        </span>
      ) : (
        <span className={styles.mute}>not tagged yet</span>
      )}
    </div>
  );
}

function JudgeOn({ d, line }: { d: Derived; line: number }) {
  const fails = d.failLines.get(line) ?? [];
  const facts = d.facts.get(line) ?? [];
  if (fails.length === 0 && facts.length === 0) return <p className={styles.empty}>The judge did not sample this line.</p>;
  return (
    <ul className={styles.facts} data-role="review-moment-judge">
      {fails.map((r) => (
        <li key={`f${r}`} className={styles.bad}>
          Failed: {ruleName(r)}
        </li>
      ))}
      {facts.map(([dim, v]) => (
        <li key={`q${dim}`} className={v ? styles.ok : styles.bad}>
          {v ? "✓" : "✗"} {dimName(dim)}
        </li>
      ))}
    </ul>
  );
}
