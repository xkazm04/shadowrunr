// brain/engine compare: a named baseline against the candidate, role by role (diff-comparison,
// pair-and-baseline-selection). The pair is named above the rows; a side read from a page
// window is said to be one and the comparison is marked partial; a role under the minimum
// sample reads "too few calls", a one-sided role "new" or "gone" - never a delta against 0.
import { int, msText, usd } from "../../kit/events";
import { Chip, type Tone } from "../../ui/Chip";
import { Section } from "../../ui/Section";
import { roleColor } from "./draw";
import { compareSummaries, signedMs, signedPp, STATE_LABEL, type DeltaCell, type Direction, type EngineSummary } from "./engineModel";
import styles from "./Layer2.module.css";

const TONE: Readonly<Record<Direction, Tone>> = { worse: "caution", better: "ok", same: "neutral" };

/** One side of the pair: its name, its summary, and how many calls a page window holds when the
 * side is not the whole ledger. */
export interface CompareSide {
  readonly label: string;
  readonly s: EngineSummary;
  readonly windowCalls: number | undefined;
  readonly hash: string | undefined;
}

const pct = (x: number | undefined) => (x === undefined ? "–" : `${x.toFixed(1)}%`);

/** A delta chip; its title carries both sides (`p50 1,000 ms → 1,400 ms`). */
function Delta({ c, unit }: { c: DeltaCell; unit: "ms" | "pp" }) {
  if (c.delta === undefined || c.direction === undefined) return <>–</>;
  const side = unit === "ms" ? msText : pct;
  return (
    <Chip tone={TONE[c.direction]} title={`${c.direction}: ${side(c.base)} → ${side(c.cand)}`}>
      {unit === "ms" ? signedMs(c.delta) : signedPp(c.delta)}
    </Chip>
  );
}

const signedUsd = (d: number) => `${d > 0 ? "+" : d < 0 ? "−" : "±"}${usd(Math.abs(d))}`;

export function EngineCompare({ base, cand }: { base: CompareSide; cand: CompareSide }) {
  const c = compareSummaries(base.s, cand.s);
  const partial = base.windowCalls !== undefined || cand.windowCalls !== undefined;
  const differ = base.hash !== undefined && cand.hash !== undefined && base.hash !== cand.hash;
  return (
    <Section title="Compare" right={partial ? "partial" : `${int(base.s.calls)} → ${int(cand.s.calls)} calls`}>
      <div data-role="brain-compare" data-partial={String(partial)}>
        <p className={styles.note} data-role="brain-compare-pair">
          <span>
            baseline <code className={styles.mono}>{base.label}</code>
          </span>
          <span aria-hidden="true">→</span>
          <span>
            candidate <code className={styles.mono}>{cand.label}</code>
          </span>
        </p>
        {partial && (
          <p className={styles.note}>
            <Chip tone="caution">partial</Chip> {base.windowCalls !== undefined && `baseline: first ${int(base.windowCalls)} calls. `}
            {cand.windowCalls !== undefined && `candidate: first ${int(cand.windowCalls)} calls. `}
            The deltas read the loaded rows, not the whole ledger.
          </p>
        )}
        {differ && (
          <p className={styles.note}>
            <Chip tone="caution">instruments differ</Chip> baseline {base.hash}, candidate {cand.hash}: compare engine behaviour within one instrument hash only.
          </p>
        )}
        <p className={styles.note}>
          Error rate <Delta c={c.total.errorRate} unit="pp" /> · fallbacks <Delta c={c.total.fallbackRate} unit="pp" /> · cost {signedUsd(c.cost.delta)}
          {c.cost.lowerBound && " (uncosted calls on a side: each cost is a lower bound)"}
        </p>
        <table className={styles.table}>
          <thead>
            <tr>
              <th>Role</th>
              <th className={styles.num}>Calls</th>
              <th className={styles.num}>Error rate</th>
              <th className={styles.num}>Fallbacks</th>
              <th className={styles.num}>p50</th>
              <th className={styles.num}>p95</th>
              <th className={styles.num}>First text p50</th>
            </tr>
          </thead>
          <tbody>
            {c.rows.map((r) => (
              <tr key={r.role} data-state={r.state}>
                <td>
                  <span className={styles.role}>
                    <i style={{ background: roleColor(r.role) }} />
                    {r.role}
                  </span>{" "}
                  {r.state !== "both" && <Chip tone="muted">{STATE_LABEL[r.state]}</Chip>}
                </td>
                <td className={styles.num}>
                  {r.baseCalls === undefined ? "–" : int(r.baseCalls)} → {r.candCalls === undefined ? "–" : int(r.candCalls)}
                </td>
                <td className={styles.num}>
                  <Delta c={r.errorRate} unit="pp" />
                </td>
                <td className={styles.num}>
                  <Delta c={r.fallbackRate} unit="pp" />
                </td>
                <td className={styles.num}>
                  <Delta c={r.p50} unit="ms" />
                </td>
                <td className={styles.num}>
                  <Delta c={r.p95} unit="ms" />
                </td>
                <td className={styles.num}>
                  <Delta c={r.firstText} unit="ms" />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  );
}
