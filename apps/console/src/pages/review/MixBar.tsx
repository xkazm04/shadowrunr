// A proportion bar of line kinds, with a key of each kind's share (optionally the top n).
import { KIND_INK, KINDS, type ShownKind } from "./names";
import styles from "./MixBar.module.css";

export type Mix = Readonly<Partial<Record<ShownKind, number>>>;

const pct = (n: number, total: number): number => (total > 0 ? Math.round((100 * n) / total) : 0);

/** A wire mix (`ReviewSessionRow.mix`) as the page's kinds; lines with no tag are `untagged`. */
export function shownMix(mix: { readonly [k: string]: number | undefined }, lines: number): Mix {
  const out: Partial<Record<ShownKind, number>> = {};
  let tagged = 0;
  for (const [k] of KINDS) {
    const n = mix[k] ?? 0;
    if (n > 0) {
      out[k] = n;
      tagged += n;
    }
  }
  if (lines > tagged) out.untagged = (out.untagged ?? 0) + lines - tagged;
  return out;
}

export function MixBar(props: { mix: Mix; big?: boolean; top?: number }) {
  const kinds = KINDS.filter(([k]) => (props.mix[k] ?? 0) > 0);
  const total = kinds.reduce((n, [k]) => n + (props.mix[k] ?? 0), 0);
  const key = [...kinds].sort((a, b) => (props.mix[b[0]] ?? 0) - (props.mix[a[0]] ?? 0)).slice(0, props.top ?? kinds.length);
  return (
    <div className={styles.mix}>
      <div
        className={props.big ? `${styles.bar} ${styles.big}` : styles.bar}
        role="img"
        aria-label={`Share of line types: ${key.map(([k, name]) => `${name} ${pct(props.mix[k] ?? 0, total)}%`).join(", ") || "no lines"}`}
      >
        {kinds.map(([k, name]) => (
          <span key={k} title={`${name}: ${props.mix[k]} lines, ${pct(props.mix[k] ?? 0, total)}%`} style={{ width: `${(100 * (props.mix[k] ?? 0)) / total}%`, background: KIND_INK[k] }} />
        ))}
      </div>
      {key.length > 0 && (
        <ul className={styles.key} aria-hidden="true">
          {key.map(([k, name]) => (
            <li key={k}>
              <i style={{ background: KIND_INK[k] }} />
              {name} <b>{pct(props.mix[k] ?? 0, total)}%</b>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
