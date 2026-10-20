// Review by type: a card per line kind in the session with how many of its lines carry the
// operator's mark. A card opens the player on that kind.
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
  return (
    <section className={kit.panel} data-role="review-types">
      <div className={kit.kicker}>Review by type</div>
      <h2 className={kit.title}>Pick a type to step through every line of it until each has your mark</h2>
      <p className={kit.lede}>
        Each line shows with what came before it and the chat it answered. Mark it and save, and the next unmarked line follows. Keys: <kbd>1</kbd> to <kbd>8</kbd> toggle a mark,{" "}
        <kbd>Enter</kbd> saves and moves on, <kbd>→</kbd> skips, <kbd>←</kbd> goes back to the line before, even one you already marked.
      </p>
      {counts.length === 0 ? (
        <p className={kit.empty}>He said nothing in this session.</p>
      ) : (
        <ul className={styles.cards}>
          {counts.map((c) => (
            <li key={c.kind}>
              <button type="button" className={styles.card} data-kind={c.kind} onClick={() => props.openPlayer(c.kind)}>
                <span className={styles.swatch} style={{ background: KIND_INK[c.kind] }} />
                <span className={styles.name}>{KIND_NAME[c.kind]}</span>
                <span className={styles.n}>
                  {c.marked} of {c.total} marked
                </span>
                <span className={styles.bar} role="img" aria-label={`${c.marked} of ${c.total} marked`}>
                  <i style={{ width: `${(100 * c.marked) / c.total}%` }} />
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
