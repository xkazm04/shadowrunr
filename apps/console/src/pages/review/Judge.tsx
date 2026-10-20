// The judge: a card per rule it failed (the planted moment, what was required, the lines it
// points at), the quality facts on the lines it sampled worst first, and its notes.
import type { ReviewSession, RuleInfo } from "@shadowrunr/contract";
import { lineOf, tsOf } from "./derive";
import styles from "./Judge.module.css";
import { dimName, fmtClock, ruleName } from "./names";
import type { TabProps } from "./tab";
import { sayNear, sentence } from "./tabkit";
import kit from "./tabkit.module.css";

export interface DimRow {
  readonly dim: string;
  readonly yes: number;
  readonly total: number;
  /** The dimension's question, from the catalog; absent when it has none. */
  readonly question?: string;
}

/** Quality facts per dimension, worst share of yes first. */
export function dimRows(rs: ReviewSession, catalog: ReadonlyMap<string, RuleInfo>): DimRow[] {
  const by = new Map<string, { yes: number; total: number }>();
  for (const q of rs.verdict?.quality ?? []) {
    const r = by.get(q.dimension) ?? { yes: 0, total: 0 };
    by.set(q.dimension, { yes: r.yes + (q.value ? 1 : 0), total: r.total + 1 });
  }
  return [...by]
    .map(([dim, r]) => {
      const question = (catalog.get(`q:${dim}`) ?? catalog.get(dim))?.required;
      return { dim, ...r, ...(question ? { question } : {}) };
    })
    .sort((a, b) => a.yes / a.total - b.yes / b.total || a.dim.localeCompare(b.dim));
}

export function Judge(props: TabProps) {
  const { rs, d } = props;
  const v = rs.verdict;
  if (!v) {
    return (
      <section className={kit.panel} data-role="review-judge">
        <div className={kit.kicker}>The judge</div>
        <h2 className={kit.title}>This session was not judged</h2>
        <p className={kit.lede}>Sessions are judged when they end. A session stopped early, or one whose judge call failed, has no verdict.</p>
      </section>
    );
  }
  const catalog = new Map(rs.catalog.map((c) => [c.id, c]));
  const fails = v.rules.filter((r) => r.state === "fail");
  const dims = dimRows(rs, catalog);
  const sampled = new Set(v.quality.map((q) => lineOf(q.turn_ref)).filter((l) => l !== undefined)).size;
  const open = (line: number) => {
    const l = sayNear(d, line);
    if (l !== undefined) props.openLine(l);
  };
  return (
    <div className={styles.judge} data-role="review-judge">
      <section className={kit.panel}>
        <div className={kit.kicker}>Rules the judge failed</div>
        <h2 className={kit.title}>{fails.length ? `${fails.length} of the ${v.rules.length} rules it checked` : "None"}</h2>
        {fails.length === 0 ? (
          <p className={kit.empty}>The judge failed no rule in this session.</p>
        ) : (
          <div className={styles.cards}>
            {fails.map((f) => {
              const c = catalog.get(f.rule);
              const refs = (f.evidence ?? []).map(lineOf).filter((l): l is number => l !== undefined);
              return (
                <article key={f.rule} className={styles.card} data-role="review-judge-fail">
                  <h3 className={styles.rule}>{ruleName(f.rule)}</h3>
                  {c?.when && (
                    <p className={styles.req}>
                      <span className={kit.label}>Planted moment</span>
                      <br />
                      {sentence(c.when)}
                    </p>
                  )}
                  {c?.required && (
                    <p className={styles.req}>
                      <span className={kit.label}>What was required</span>
                      <br />
                      {sentence(c.required)}
                    </p>
                  )}
                  <div className={styles.refs}>
                    <span className={kit.label}>{refs.length ? "The judge points at" : "The judge cited no line"}</span>
                    {refs.map((l) => {
                      const e = rs.events[l - 1];
                      return (
                        <button key={l} type="button" className={kit.btn} data-line={l} onClick={() => open(l)}>
                          {e ? fmtClock(tsOf(e)) : "–"} · line {l}
                        </button>
                      );
                    })}
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </section>
      <div className={kit.stack}>
        <section className={kit.panel}>
          <div className={kit.kicker}>Quality on sampled lines</div>
          <h2 className={kit.title}>
            {sampled} of {d.says.length} lines sampled
          </h2>
          {dims.length === 0 ? (
            <p className={kit.empty}>The judge recorded no quality facts.</p>
          ) : (
            <ul className={styles.dims}>
              {dims.map((r) => (
                <li key={r.dim} className={styles.dim} data-role="review-judge-dim">
                  <span title={r.question}>{dimName(r.dim)}</span>
                  <span className={styles.qbar} role="img" aria-label={`${r.yes} yes, ${r.total - r.yes} no`}>
                    <i style={{ width: `${(100 * r.yes) / r.total}%` }} />
                  </span>
                  <span className={kit.mono}>
                    {r.yes}/{r.total}
                  </span>
                </li>
              ))}
            </ul>
          )}
          <p className={styles.aside}>Line types come from the tagger, not from the judge.</p>
        </section>
        {v.notes && (
          <section className={kit.panel}>
            <div className={kit.kicker}>The judge’s notes</div>
            <p className={styles.notes}>{v.notes}</p>
          </section>
        )}
      </div>
    </div>
  );
}
