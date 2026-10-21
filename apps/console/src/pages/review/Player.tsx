// The player: every line of one kind in transcript order, one at a time, until each has the
// operator's mark, with the session's recording seeked to the line on request (WatchPane). The
// player's keys are the `review.player` scope in the one keymap (keys/keymap.ts): they hold while
// focus is inside the player (focused on mount and on every step), shadowing the shell's bare 1-7
// page jumps there, as the conflict report lists; outside the player the keymap keeps its meaning,
// and in the note only Ctrl+Enter (save) fires.
import { useEffect, useRef, useState } from "react";
import { useKeyScope } from "../../kit/keyCommands";
import { pageRoute, useRouter } from "../../router";
import { Button } from "../../ui/Button";
import { RateBar } from "../../ui/RateBar";
import { EmptyState } from "../../ui/States";
import { Switch } from "../../ui/Switch";
import type { SayLine } from "./derive";
import { FeedbackPanel, type FeedbackHandle } from "./FeedbackPanel";
import { KIND_NAME, type ShownKind } from "./names";
import styles from "./Player.module.css";
import { FailedMarks, PlayerDone, PlayerSide, TagLine } from "./PlayerParts";
import { Script } from "./Script";
import type { TabProps } from "./tab";
import { WatchPane } from "./WatchPane";

/** Where the player stands: an index into the kind's lines, or the done state. */
interface At {
  readonly i: number;
  /** ← landed here: show this line even when it is marked (for this step only). */
  readonly hold: boolean;
  readonly done: boolean;
}

/** The first index from `from` on (wrapping once) that `ok` accepts, or -1. */
function seek(n: number, from: number, ok: (i: number) => boolean, wrap: boolean): number {
  for (let k = 0; k < n; k++) {
    const j = from + k;
    if (j >= n && !wrap) return -1;
    if (ok(j % n)) return j % n;
  }
  return -1;
}

export function Player(props: TabProps) {
  if (!props.kind) return <EmptyState title="No type picked" hint="Pick a type on Review by type to start the player." />;
  // A new kind starts a new walk.
  return <Walk key={props.kind} {...props} kind={props.kind} />;
}

function Walk(props: TabProps & { kind: ShownKind }) {
  const { rs, d, feedback, kind } = props;
  const list: readonly SayLine[] = d.says.filter((s) => s.kind === kind);
  const n = list.length;
  const [skip, setSkip] = useState(true);
  const [at, setAt] = useState<At>({ i: 0, hold: false, done: false });
  const router = useRouter();
  const panel = useRef<FeedbackHandle>(null);
  const root = useRef<HTMLDivElement>(null);
  // A save advances as soon as it is sent, before its optimistic write re-rendered: `next` reads
  // the marks through this ref and takes the saved line's own state from the panel.
  const marks = useRef(feedback.byLine);
  useEffect(() => {
    marks.current = feedback.byLine;
  });
  const marked = (i: number) => marks.current.has(list[i]?.line ?? -1);
  const shown = (i: number) => feedback.byLine.has(list[i]?.line ?? -1);
  const markedNow = list.filter((s) => feedback.byLine.has(s.line)).length;

  // The line on screen: skip-marked moves past a marked line unless ← just landed on it.
  let cur = -1;
  if (!at.done && n > 0) {
    const i = Math.min(at.i, n - 1);
    cur = !skip || at.hold || !shown(i) ? i : seek(n, i, (j) => !shown(j), true);
  }
  const line = cur >= 0 ? list[cur] : undefined;

  useEffect(() => {
    const el = root.current;
    if (el && !el.contains(document.activeElement)) el.focus({ preventScroll: true });
  }, [line?.line]);

  const go = (i: number) => setAt(i < 0 ? { i: 0, hold: false, done: true } : { i, hold: false, done: false });
  /** After a save is sent: the next line the filter accepts, else any unmarked line, else done. */
  const next = (savedMarked: boolean) => {
    const m = (j: number) => (j === cur ? savedMarked : marked(j));
    const after = seek(n, cur + 1, (j) => !skip || !m(j), false);
    go(after >= 0 ? after : seek(n, 0, (j) => !m(j), false));
  };
  const skipOn = () => go(n === 0 ? -1 : seek(n, cur + 1, (j) => !skip || !marked(j) || j === cur, true));
  const back = () => setAt({ i: Math.max(0, cur - 1), hold: true, done: false });

  useKeyScope("review.player", {
    toggle: (i) => panel.current?.toggle(i),
    save: () => panel.current?.save(),
    skip: skipOn,
    back,
  });

  if (!line) {
    return (
      <>
        <FailedMarks feedback={feedback} />
        <PlayerDone
          kind={kind}
          list={list}
          feedback={feedback}
          onPick={() => router.navigate(pageRoute("review", rs.row.session_id, "types"))}
          onAgain={() => {
            setSkip(false);
            go(0);
          }}
        />
      </>
    );
  }
  return (
    <div ref={root} className={styles.player} tabIndex={-1} data-key-scope="review.player" data-role="review-player" data-line={line.line}>
      <section className={styles.stage}>
        <div className={styles.count}>
          <div>
            <div className={styles.pos} data-role="review-player-pos">
              {KIND_NAME[kind]} {cur + 1} of {n}
            </div>
            <span className={styles.big}>
              {markedNow} marked · {n - markedNow} to go
            </span>
          </div>
          <Switch checked={skip} onChange={setSkip} label="Skip lines I already marked" />
        </div>
        <FailedMarks feedback={feedback} />
        <RateBar rate={markedNow / Math.max(1, n)} n={n} label={`${markedNow} of ${n} marked`} />
        <WatchPane sessionId={rs.row.session_id} line={line.line} t={line.t} />
        <div key={line.line} className={styles.step} data-role="review-player-step">
          <Script rs={rs} d={d} line={line.line} context={2} />
          <TagLine say={line} />
        </div>
        <FeedbackPanel
          ref={panel}
          line={line.line}
          feedback={feedback}
          keys
          saveLabel="Save and next"
          onSaved={next}
          before={
            <>
              <Button onClick={back} disabled={cur === 0}>
                Previous
              </Button>
              <Button onClick={skipOn}>Skip</Button>
            </>
          }
        />
        <p className={styles.keys}>
          <kbd>1</kbd>–<kbd>8</kbd> toggle a mark · <kbd>Enter</kbd> save and next · <kbd>→</kbd> skip · <kbd>←</kbd> back · in the note, <kbd>Ctrl</kbd>+<kbd>Enter</kbd> saves
        </p>
      </section>
      <PlayerSide rs={rs} d={d} list={list} cur={cur} marked={(l) => feedback.byLine.has(l)} openLine={props.openLine} />
    </div>
  );
}
