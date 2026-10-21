// Watched feedback on one cut: what a person saw when they played it (tags and a note),
// stored with the clip by `clips.feedback`. It changes no state and holds nothing; it is how
// a cut is judged as a viewer sees it.
import type { ClipCandidate, ClipFeedbackTag } from "@shadowrunr/contract";
import { useState } from "react";
import { useMutation, viewOfError } from "../../data";
import { Button } from "../../ui/Button";
import { Pill, pillRow } from "../../ui/PillGroup";
import { Section } from "../../ui/Section";
import { FEEDBACK_TAGS } from "./clipModel";
import styles from "./ClipPlayer.module.css";

export function ClipFeedback({ c }: { c: ClipCandidate }) {
  const save = useMutation("clips.feedback");
  const [tags, setTags] = useState<ClipFeedbackTag[]>(c.feedback?.tags ?? []);
  const [note, setNote] = useState(c.feedback?.note ?? "");
  const [status, setStatus] = useState<string | undefined>();
  const toggle = (t: ClipFeedbackTag) => setTags((xs) => (xs.includes(t) ? xs.filter((x) => x !== t) : [...xs, t]));
  const onSave = async () => {
    setStatus(undefined);
    const trimmed = note.trim();
    const r = await save.run({ id: c.id, tags, ...(trimmed ? { note: trimmed } : {}) });
    setStatus(r.ok ? "Saved" : `${viewOfError(r.error).title}: ${r.error.message}`);
  };
  const when = c.feedback ? new Date(c.feedback.at_ms).toISOString().slice(0, 16).replace("T", " ") : undefined;
  return (
    <Section title="Watched" right={when ? `last saved ${when}` : "not watched yet"}>
      <div className={styles.fb} data-role="onair-clip-feedback">
        <div className={pillRow} role="group" aria-label="What you saw">
          {FEEDBACK_TAGS.map(([id, name]) => (
            <Pill key={id} pressed={tags.includes(id)} onToggle={() => toggle(id)}>
              {name}
            </Pill>
          ))}
        </div>
        <textarea value={note} maxLength={2000} aria-label="Note" placeholder="What did you see? Where should the cut open or close?" onChange={(e) => setNote(e.target.value)} />
        <div className={styles.actions}>
          <Button variant="ink" disabled={save.pending} onClick={() => void onSave()}>
            {save.pending ? "Saving…" : "Save feedback"}
          </Button>
          {status && (
            <span className={status === "Saved" ? styles.ok : styles.err} role="status">
              {status}
            </span>
          )}
        </div>
      </div>
    </Section>
  );
}
