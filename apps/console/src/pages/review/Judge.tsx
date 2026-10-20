// The judge: the rules it failed as evidence (the planted moment, what was required, the lines it
// points at as ref chips), the quality facts on the lines it sampled worst first as rate bars, and
// its notes in serif italic.
import type { ReviewSession, RuleInfo } from "@shadowrunr/contract";
import { RateBar } from "../../ui/RateBar";
import { Section } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
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
      <div data-role="review-judge" data-rise>
        <EmptyState title="This session was not judged" hint="Sessions are judged when they end. A session stopped early, or one whose judge call failed, has no verdict." />
      </div>
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
    <div className={styles.judge} data-role="review-judge" data-rise>
      <Section title="Rules it failed" right={fails.length ? `${fails.length} of the ${v.rules.length} rules it checked` : `None of ${v.rules.length}`}>
        {fails.length === 0 ? (
          <EmptyState title="No rule failed" hint="The judge failed no rule in this session." />
        ) : (
          <ul className={styles.evid}>
            {fails.map((f) => {
              const c = catalog.get(f.rule);
              const refs = (f.evidence ?? []).map(lineOf).filter((l): l is number => l !== undefined);
              return (
                <li key={f.rule} data-role="review-judge-fail">
                  <h3 className={styles.rule}>{ruleName(f.rule)}</h3>
                  {c?.when && (
                    <p className={styles.req}>
                      <b className={styles.label}>Planted moment</b> {sentence(c.when)}
                    </p>
                  )}
                  {c?.required && (
                    <p className={styles.req}>
                      <b className={styles.label}>What was required</b> {sentence(c.required)}
                    </p>
                  )}
                  <div className={styles.refs}>
                    <b className={styles.label}>{refs.length ? "The judge points at" : "The judge cited no line"}</b>
                    {refs.map((l) => {
                      const e = rs.events[l - 1];
                      return (
                        <button key={l} type="button" className={styles.ref} data-line={l} onClick={() => open(l)}>
                          {e ? fmtClock(tsOf(e)) : "–"} · line {l}
                        </button>
                      );
                    })}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </Section>
      <div>
        <Section title="Quality on sampled lines" right={`${sampled} of ${d.says.length} lines sampled`}>
          {dims.length === 0 ? (
            <EmptyState title="No quality facts" hint="The judge recorded no quality facts for this session." />
          ) : (
            <ul className={styles.qrows}>
              {dims.map((r) => (
                <li key={r.dim} className={styles.qrow} data-role="review-judge-dim">
                  <span title={r.question}>{dimName(r.dim)}</span>
                  <RateBar rate={r.yes / r.total} n={r.total} label={dimName(r.dim)} />
                  <span className={kit.mono}>
                    {r.yes}/{r.total}
                  </span>
                </li>
              ))}
            </ul>
          )}
          <p className={kit.note}>Line types come from the tagger, not from the judge.</p>
        </Section>
        {v.notes && (
          <Section title="The judge’s notes">
            <p className={styles.notes}>{v.notes}</p>
          </Section>
        )}
      </div>
    </div>
  );
}
