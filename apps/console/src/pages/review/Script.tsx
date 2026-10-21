// One moment typeset as a script: the lines before it (muted), the chat it answered, every line of
// its beat with the focused one highlighted, and the gates that touched it. Time, speaker and
// line sit on one grid; his voice is serif italic, chat is sans, time is mono.
import type { ReviewSession, StreamEvent } from "@shadowrunr/contract";
import type { Derived } from "./derive";
import { tsOf } from "./derive";
import { fmtClock, human, modName, RANK_NAME } from "./names";
import styles from "./Script.module.css";

/** Lines within this many seconds of the focused line belong to its beat. */
const BEAT_S = 0.05;

export function Script(props: { rs: ReviewSession; d: Derived; line: number; context?: number; compact?: boolean }) {
  const { rs, d, line } = props;
  const ev = rs.events;
  const i = line - 1;
  const focus = ev[i];
  if (!focus) return null;
  const t = tsOf(focus);
  const says = (e: StreamEvent): e is Extract<StreamEvent, { type: "say" }> => e.type === "say";
  const beat: { line: number; e: Extract<StreamEvent, { type: "say" }> }[] = [];
  ev.forEach((e, j) => { if (says(e) && Math.abs(tsOf(e) - t) < BEAT_S) beat.push({ line: j + 1, e }); });
  const before: { line: number; e: Extract<StreamEvent, { type: "say" }> }[] = [];
  for (let j = i - 1; j >= 0 && before.length < (props.context ?? 2); j--) {
    const e = ev[j];
    if (e && says(e) && Math.abs(tsOf(e) - t) >= BEAT_S) before.unshift({ line: j + 1, e });
  }
  const answered = [...new Set(beat.flatMap((b) => b.e.replying_to ?? []))].map((id) => d.chatById.get(id)).filter((c) => c !== undefined);
  const gates = ev.slice(Math.max(0, i - 10), i + 3).filter((e): e is Extract<StreamEvent, { type: "moderation" }> => e.type === "moderation" && e.stage === "output" && e.rule !== "engine_stall");
  const who = (e: Extract<StreamEvent, { type: "say" }>) =>
    e.speaker === "host" ? <span className={`${styles.who} ${styles.mal}`}>Malakor</span> : <span className={styles.who}>{human(e.foil ?? "foil")}<span className={styles.rank}>foil</span></span>;
  return (
    <div className={`${styles.script} ${props.compact ? styles.compact : ""}`} data-role="review-script">
      {before.map(({ line: l, e }) => (
        <Row key={`b${l}`} time={tsOf(e)} who={who(e)}>
          <p className={`${styles.voice} ${styles.muted} ${e.speaker === "foil" ? styles.foil : ""}`}>{e.text}</p>
        </Row>
      ))}
      {before.length > 0 && <span className={styles.sep} />}
      {answered.map((c) => (
        <Row key={`c${c.line}`} time={c.t} who={<span className={styles.who}>{c.msg.viewer.name}<span className={styles.rank}>{RANK_NAME[c.msg.viewer.rank] ?? c.msg.viewer.rank} · chat</span></span>}>
          <p className={styles.chat}>{c.msg.text}</p>
        </Row>
      ))}
      {beat.map(({ line: l, e }) => (
        <Row key={`s${l}`} time={tsOf(e)} who={who(e)}>
          <p className={`${styles.voice} ${e.speaker === "foil" ? styles.foil : ""} ${l === line ? styles.focus : ""}`} aria-current={l === line ? "true" : undefined}>{e.text}</p>
        </Row>
      ))}
      {gates.map((g, k) => {
        const m = modName(g.stage, g.rule, g.action);
        return <p key={`g${k}`} className={styles.gate}>{m.name}{m.detail ? ` · ${m.detail}` : ""}</p>;
      })}
    </div>
  );
}

function Row(props: { time: number; who: React.ReactNode; children: React.ReactNode }) {
  return (
    <>
      <span className={styles.time}>{fmtClock(props.time)}</span>
      {props.who}
      {props.children}
    </>
  );
}
