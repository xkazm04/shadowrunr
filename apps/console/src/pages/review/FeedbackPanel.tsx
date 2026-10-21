// Quick feedback on one line: toggle marks (pills), an optional note, save (the ink button). The player variant shows key
// numbers and lets its caller bind keys through the returned handle; there the note's Ctrl+Enter is
// the player scope's binding in the one keymap, so the panel binds it only on its own.
import { forwardRef, useEffect, useImperativeHandle, useState, type ReactNode } from "react";
import type { FeedbackTag } from "@shadowrunr/contract";
import { Button } from "../../ui/Button";
import { Pill, pillRow } from "../../ui/PillGroup";
import type { Feedback } from "./hooks";
import { FEEDBACK } from "./names";
import styles from "./FeedbackPanel.module.css";

const STATUS_TEXT = {
  sending: "Saving…",
  saved: "Saved",
  failed: "Your mark was not saved. Check that the daemon is running, then try again.",
} as const;

export interface FeedbackHandle {
  /** Toggle the n-th mark (0-based, in FEEDBACK order). */
  toggle: (index: number) => void;
  save: () => void;
}

export const FeedbackPanel = forwardRef<FeedbackHandle, {
  line: number;
  feedback: Feedback;
  /** Show 1-8 key hints on the marks. */
  keys?: boolean;
  /** Label of the save button ("Save", "Save and next"). */
  saveLabel?: string;
  /** Called once a save is sent (not when the daemon answers): whether the line now carries a mark. */
  onSaved?: (marked: boolean) => void;
  /** Buttons placed before the save button (the player's Previous / Skip). */
  before?: ReactNode;
}>(function FeedbackPanel(props, ref) {
  const current = props.feedback.byLine.get(props.line);
  const [tags, setTags] = useState<FeedbackTag[]>(current?.tags ?? []);
  const [note, setNote] = useState(current?.note ?? "");
  useEffect(() => {
    setTags(current?.tags ?? []);
    setNote(current?.note ?? "");
    // Reset only when the line changes, not on every optimistic write.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.line]);
  const toggle = (t: FeedbackTag) => setTags((xs) => (xs.includes(t) ? xs.filter((x) => x !== t) : [...xs, t]));
  const status = props.feedback.statusOf(props.line);
  // A line with a write in flight takes no second one (the hook refuses it too, for keys pressed in one tick).
  const save = () => {
    if (props.feedback.save(props.line, tags, note) === undefined) return;
    props.onSaved?.(tags.length > 0 || note.trim() !== "");
  };
  useImperativeHandle(ref, () => ({ toggle: (i) => { const t = FEEDBACK[i]; if (t) toggle(t[0]); }, save }));
  return (
    <section className={styles.fb} data-role="review-feedback" aria-label="Your feedback">
      <h3 className={styles.kicker}>Your feedback</h3>
      <div className={pillRow}>
        {FEEDBACK.map(([id, name], i) => (
          <Pill key={id} pressed={tags.includes(id)} onToggle={() => toggle(id)}>
            {name}
            {props.keys && <kbd className={styles.kbd}>{i + 1}</kbd>}
          </Pill>
        ))}
      </div>
      <label className={styles.label} htmlFor={`fb-note-${props.line}`}>Note, optional</label>
      <textarea
        id={`fb-note-${props.line}`}
        value={note}
        maxLength={600}
        placeholder="What should he have said instead?"
        onChange={(e) => setNote(e.target.value)}
        onKeyDown={props.keys ? undefined : (e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); save(); } }}
      />
      <div className={styles.actions}>
        {props.before}
        <Button variant="ink" disabled={status === "sending"} onClick={save}>{props.saveLabel ?? "Save"}</Button>
        {status !== "idle" && <span className={status === "failed" ? styles.err : styles.ok} role="status" data-status={status}>{STATUS_TEXT[status]}</span>}
      </div>
    </section>
  );
});
