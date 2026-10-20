// Review by type: a card per line kind in the session with how many of its lines carry the
// operator's mark, as a rate bar. A card opens the player on that kind.
import { RateBar } from "../../ui/RateBar";
import { Section } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import type { Derived } from "./derive";
import type { Feedback } from "./hooks";
import { KIND_INK } from "./ink";
import { KIND_NAME, KINDS, type ShownKind } from "./names";
import styles from "./ReviewByType.module.css";
import type { TabProps } from "./tab";
import kit from "./tabkit.module.css";

export interface KindCount {
  readonly kind: ShownKind;
  readonly total: number;
  readonly marked: number;
}

/** Every kind present in the session, in display order, with its marked count. */
export function kindCounts(d: Derived, feedback: Feedback): KindCount[] {
  const marked = new Map<ShownKind, number>();
  for (const s of d.says) if (feedback.byLine.has(s.line)) marked.set(s.kind, (marked.get(s.kind) ?? 0) + 1);
  return KINDS.flatMap(([kind]) => {
    const total = d.mix[kind] ?? 0;
    return total > 0 ? [{ kind, total, marked: marked.get(kind) ?? 0 }] : [];
  });
}

export function ReviewByType(props: TabProps) {
  const counts = kindCounts(props.d, props.feedback);
  const marked = counts.reduce((n, c) => n + c.marked, 0);
  const total = counts.reduce((n, c) => n + c.total, 0);
  return (
    <div data-role="review-types">
      <Section title="Pick a type" right={`${marked} of ${total} lines marked`}>
        <p className={kit.note}>
          Each line shows with what came before it and the chat it answered. Mark it and save, and the next unmarked line follows. Keys: <kbd>1</kbd> to <kbd>8</kbd> toggle a mark,{" "}
          <kbd>Enter</kbd> saves and moves on, <kbd>→</kbd> skips, <kbd>←</kbd> goes back to the line before, even one you already marked.
        </p>
        {counts.length === 0 ? (
          <EmptyState title="No lines" hint="He said nothing in this session." />
        ) : (
          <ul className={styles.cards}>
            {counts.map((c) => (
              <li key={c.kind}>
                <button type="button" className={styles.card} data-kind={c.kind} onClick={() => props.openPlayer(c.kind)}>
                  <span className={styles.head}>
                    <span className={styles.swatch} style={{ background: KIND_INK[c.kind] }} />
                    <span className={styles.name}>{KIND_NAME[c.kind]}</span>
                  </span>
                  <span className={styles.n}>
                    {c.marked} of {c.total} marked
                  </span>
                  <RateBar rate={c.marked / c.total} n={c.total} label={`${KIND_NAME[c.kind]} marked`} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </div>
  );
}
