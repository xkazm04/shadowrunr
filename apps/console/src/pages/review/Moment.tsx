// The timeline's detail, in the shared ui/Drawer (wide, in Review's palette). Two views: a row's
// lines (time, kind, his words, ✓ when marked), and one moment: the script around the line, its
// tag, the judge on it and your feedback. The view's body is keyed, so it rises again when the
// drawer moves from the row's lines to a moment and back. The drawer handles Esc and focus.
import type { ReactNode } from "react";
import type { ReviewSession } from "@shadowrunr/contract";
import { pageRoute, RouteLink } from "../../router";
import { Button, buttonClass } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import { Drawer } from "../../ui/Drawer";
import { EmptyState } from "../../ui/States";
import { visibleSays, type Bucket } from "./buckets";
import { segmentAt, tsOf, type Derived, type SayLine } from "./derive";
import { FeedbackPanel } from "./FeedbackPanel";
import type { Feedback } from "./hooks";
import { dimName, EDGE_NAME, fmtClock, KIND_INK, KIND_NAME, ruleName, SEGMENT_NAME, TARGET_NAME, type ShownKind } from "./names";
import styles from "./Moment.module.css";
import { Script } from "./Script";
import kit from "./tabkit.module.css";

export interface MomentProps {
  readonly rs: ReviewSession;
  readonly d: Derived;
  readonly feedback: Feedback;
  readonly open: boolean;
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

interface View {
  readonly key: string;
  readonly kicker: ReactNode;
  readonly title: ReactNode;
  readonly actions?: ReactNode;
  readonly body: ReactNode;
}

export function Moment(props: MomentProps) {
  const view = props.line !== undefined ? momentView(props, props.line) : props.bucket ? rowView(props, props.bucket) : undefined;
  return (
    <Drawer
      open={props.open && view !== undefined}
      onClose={props.onClose}
      page="review"
      size="wide"
      closeLabel="Close details"
      data-role="review-drawer"
      kicker={view?.kicker}
      title={view?.title ?? ""}
      actions={view?.actions}
    >
      {view && (
        <div key={view.key} className={kit.stack}>
          {view.body}
        </div>
      )}
    </Drawer>
  );
}

function rowView(props: MomentProps, b: Bucket): View {
  const vis = visibleSays(b, props.shown);
  const nv = vis.filter((s) => props.feedback.byLine.has(s.line)).length;
  const seg = segName(props.rs, (b.from + b.to) / 2);
  return {
    key: `r${b.i}`,
    kicker: `${fmtClock(b.from)} to ${fmtClock(b.to)}${seg ? ` · ${seg}` : ""}`,
    title: `${vis.length} of ${b.says.length} lines shown · ${nv} marked`,
    body:
      vis.length === 0 ? (
        <EmptyState title="No lines of the shown types here" hint="Widen the type filter above the timeline to list them." />
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
      ),
  };
}

function momentView(props: MomentProps, line: number): View | undefined {
  const { rs, d, bucket: b } = props;
  const e = rs.events[line - 1];
  if (!e) return undefined;
  const t = tsOf(e);
  const seg = segName(rs, t);
  const say = d.sayByLine.get(line);
  return {
    key: `m${line}`,
    kicker: (
      <span data-role="review-moment-kicker">
        {fmtClock(t)}
        {seg ? ` · ${seg}` : ""} · line {line}
      </span>
    ),
    title: "The moment",
    ...(b
      ? {
          actions: (
            <Button size="compact" onClick={props.onBack} data-role="review-drawer-back">
              All lines in {fmtClock(b.from)}–{fmtClock(b.to)}
            </Button>
          ),
        }
      : {}),
    body: (
      <>
        <Script rs={rs} d={d} line={line} />
        {say ? <TagMeta say={say} /> : <p className={kit.note}>This is not one of Malakor’s lines; the timeline marks only his.</p>}
        <section aria-label="The judge on this line">
          <h3 className={kit.sub}>Judge</h3>
          <JudgeOn d={d} line={line} />
        </section>
        {say && <FeedbackPanel line={line} feedback={props.feedback} />}
        {say && <ToToneLab rs={rs} line={line} />}
      </>
    ),
  };
}

/** "Send to Tone Lab": the character's Tone Lab with this line and its situation prefilled. */
function ToToneLab({ rs, line }: { rs: ReviewSession; line: number }) {
  const start = rs.events.find((e) => e.type === "stream_start");
  if (start?.type !== "stream_start") return null;
  return (
    <div>
      <RouteLink to={pageRoute("characters", start.character, "package", "tone-lab", rs.row.session_id, String(line))} className={buttonClass("secondary", "compact")} data-role="review-to-tone-lab">
        Send to Tone Lab
      </RouteLink>
    </div>
  );
}

function TagMeta({ say }: { say: SayLine }) {
  const tag = say.tag;
  return (
    <div className={styles.meta} data-role="review-moment-tag">
      <Chip>
        <span className={styles.sw} style={{ background: KIND_INK[say.kind] }} /> {KIND_NAME[say.kind]}
      </Chip>
      {say.kind === "stall" ? (
        <span className={styles.aim}>derived from the engine record</span>
      ) : tag ? (
        <span className={styles.aim}>
          aimed at {TARGET_NAME[tag.target]} · {EDGE_NAME[tag.edge]}
          {tag.why ? ` · “${tag.why}”` : ""}
        </span>
      ) : (
        <span className={styles.aim}>not tagged yet</span>
      )}
    </div>
  );
}

/** The judge on one line, as evidence (reconcile .evid): failed rules and facts on a fail rule, held facts on an ok rule. */
export function JudgeOn({ d, line }: { d: Derived; line: number }) {
  const fails = d.failLines.get(line) ?? [];
  const facts = d.facts.get(line) ?? [];
  if (fails.length === 0 && facts.length === 0) return <p className={kit.note}>The judge did not sample this line.</p>;
  return (
    <ul className={styles.evid} data-role="review-moment-judge">
      {fails.map((r) => (
        <li key={`f${r}`} className={styles.evidE}>
          Failed: {ruleName(r)}
        </li>
      ))}
      {facts.map(([dim, v]) => (
        <li key={`q${dim}`} className={v ? undefined : styles.evidE}>
          {v ? "✓" : "✗"} {dimName(dim)}
        </li>
      ))}
    </ul>
  );
}
