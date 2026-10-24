// One timeline row, one line high and full width: time, segment band, Malakor's lines as dots by
// kind (voted dots ringed and checked), chat density, show marks, gates and judge, your marks.
// A fold stands in for a run of quiet rows.
import { memo } from "react";
import type { SegmentId } from "@shadowrunr/contract";
import { Chip, type Tone } from "../../ui/Chip";
import { mergeMarks, visibleSays, type Bucket } from "./buckets";
import { fmtClock, GATE_MARK_NAME, KIND_INK, KIND_NAME, SEGMENT_NAME, SHOW_MARK_NAME, type GateMarkId, type ShowMarkId, type ShownKind } from "./names";
import styles from "./Timeline.module.css";

/** Dots per row before "+n". */
export const DOT_CAP = 40;
/** Show marks per row before "+n". */
const MARK_CAP = 3;

const MARK_TONE: Readonly<Partial<Record<ShowMarkId, Tone>>> = { engine_down: "fail", engine_degraded: "fail", engine_back: "ok" };
const GATE_TONE: Readonly<Record<GateMarkId, Tone>> = { care: "fail", persona: "fail", alleged: "neutral", judge_fail: "fail" };
const GATES: readonly Exclude<GateMarkId, "judge_fail">[] = ["care", "persona", "alleged"];

export function segmentClass(seg: SegmentId | string | undefined): string | undefined {
  if (seg === "SEG-01") return styles.segImprov;
  if (seg === "SEG-02A") return styles.segInvestigation;
  if (seg === "SEG-03") return styles.segTableRead;
  return seg ? styles.segOther : undefined;
}

export function TimelineHead() {
  return (
    <div className={`${styles.cols} ${styles.head}`} aria-hidden="true">
      <span className={styles.label}>Time</span>
      <span />
      <span className={styles.label}>Malakor’s lines, by type</span>
      <span className={styles.label}>Chat</span>
      <span className={styles.label}>Show</span>
      <span className={`${styles.label} ${styles.cGate}`}>Gates · judge</span>
      <span className={`${styles.label} ${styles.cYou}`}>Your marks</span>
    </div>
  );
}

export interface RowProps {
  readonly b: Bucket;
  readonly shown: ReadonlySet<ShownKind>;
  readonly voted: ReadonlySet<number>;
  readonly maxChat: number;
  readonly segment: string | undefined;
  readonly selected: boolean;
  /** The row a deep link landed on: it flashes once. */
  readonly flash: boolean;
  readonly onOpen: (row: number) => void;
}

export const TimelineRow = memo(function TimelineRow({ b, shown, voted, maxChat, segment, selected, flash, onOpen }: RowProps) {
  const vis = visibleSays(b, shown);
  const nv = vis.filter((s) => voted.has(s.line)).length;
  const marks = mergeMarks(b.show);
  const segName = segment ? SEGMENT_NAME[segment] : undefined;
  return (
    <button
      type="button"
      className={`${styles.cols} ${styles.row}${flash ? ` ${styles.flash}` : ""}`}
      aria-current={selected ? "true" : undefined}
      data-flash={flash || undefined}
      aria-label={`${fmtClock(b.from)}: ${vis.length} lines shown${nv ? `, ${nv} marked` : ""}${marks.length ? `, ${marks.map((m) => SHOW_MARK_NAME[m.id]).join(", ")}` : ""}`}
      data-row={b.i}
      data-role="review-tl-row"
      onClick={() => onOpen(b.i)}
    >
      <span className={styles.t}>{fmtClock(b.from)}</span>
      <span className={`${styles.band} ${segmentClass(segment) ?? ""}`} title={segName} />
      <span className={styles.dots}>
        {vis.slice(0, DOT_CAP).map((s) => {
          const v = voted.has(s.line);
          return (
            <span
              key={s.line}
              className={v ? `${styles.dot} ${styles.voted}` : styles.dot}
              style={{ background: KIND_INK[s.kind] }}
              title={`${KIND_NAME[s.kind]}${v ? " · you gave feedback" : ""}: ${s.text.slice(0, 100)}`}
              data-line={s.line}
              data-voted={v || undefined}
            />
          );
        })}
        {vis.length > DOT_CAP && <span className={styles.more}>+{vis.length - DOT_CAP}</span>}
        {vis.length === 0 && <span className={styles.more}>{b.says.length ? `${b.says.length} lines of hidden types` : "no lines"}</span>}
      </span>
      <span className={styles.dens} title={`${b.chat} chat messages`}>
        <i style={{ width: `${(100 * b.chat) / maxChat}%` }} />
      </span>
      <span className={styles.marks}>
        {marks.slice(0, MARK_CAP).map((m) => (
          <Chip key={m.id} tone={MARK_TONE[m.id] ?? "neutral"}>
            {SHOW_MARK_NAME[m.id]}
            {m.n > 1 ? ` ×${m.n}` : ""}
          </Chip>
        ))}
        {marks.length > MARK_CAP && <span className={styles.more}>+{marks.length - MARK_CAP}</span>}
      </span>
      <span className={`${styles.marks} ${styles.cGate}`}>
        {GATES.filter((g) => b.gates[g]).map((g) => (
          <Chip key={g} tone={GATE_TONE[g]}>
            {GATE_MARK_NAME[g]}
            {(b.gates[g] ?? 0) > 1 ? ` ${b.gates[g]}` : ""}
          </Chip>
        ))}
        {b.fails > 0 && <Chip tone={GATE_TONE.judge_fail}>{GATE_MARK_NAME.judge_fail}</Chip>}
      </span>
      <span className={`${styles.you} ${styles.cYou} ${vis.length && nv === vis.length ? styles.all : ""}`} data-role="review-tl-you">
        {nv ? `✓ ${nv}/${vis.length}` : vis.length ? `– /${vis.length}` : ""}
      </span>
    </button>
  );
});

export function TimelineFold({ from, to }: { from: number; to: number }) {
  return (
    <div className={styles.fold} data-role="review-tl-fold">
      <span className={styles.t}>{fmtClock(from)}</span>
      <span>{fmtClock(to - from)} with nothing of the shown types, folded</span>
    </div>
  );
}
