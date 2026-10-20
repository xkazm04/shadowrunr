// Engine calls in three steps: by role, then calls per minute, then the rows of one minute 20 at
// a time. The whole ledger is never listed at once. Every optional ledger field is read as
// possibly absent: the sandbox ledger fills what its engine reported.
import { useState } from "react";
import type { LedgerEntry, ReviewSession, Role } from "@shadowrunr/contract";
import { Section } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import styles from "./EngineCalls.module.css";
import { fmtClock, human, ROLE_NAME, TIER_NAME } from "./names";
import type { TabProps } from "./tab";
import { PAGE, Pager, pct, secs } from "./tabkit";
import kit from "./tabkit.module.css";

/** A ledger stamp below this is already stream milliseconds; above it, Unix milliseconds. */
const UNIX_MS = 1e11;

/** A call's stream seconds: Unix stamps are taken from the session's start. */
export function callAt(rs: ReviewSession, c: LedgerEntry): number {
  const ms = c.ts_ms >= UNIX_MS ? c.ts_ms - rs.row.started_ms : c.ts_ms;
  return Math.max(0, ms / 1000);
}

export interface RoleRow {
  readonly name: string;
  readonly calls: number;
  readonly errors: number;
  readonly p50?: number;
  readonly p95?: number;
  readonly firstP50?: number;
  /** Absent when no call reported a cost. */
  readonly cost?: number;
}

/** Step 1: per role, most calls first. */
export function roleRows(calls: readonly LedgerEntry[]): RoleRow[] {
  const by = new Map<Role, LedgerEntry[]>();
  for (const c of calls) by.set(c.role, [...(by.get(c.role) ?? []), c]);
  return [...by]
    .sort((a, b) => b[1].length - a[1].length)
    .map(([role, xs]) => {
      const dur = xs.map((c) => c.duration_ms);
      const first = xs.flatMap((c) => (c.first_text_ms === undefined ? [] : [c.first_text_ms]));
      const costs = xs.flatMap((c) => (c.cost_usd === undefined ? [] : [c.cost_usd]));
      const p50 = pct(dur, 0.5);
      const p95 = pct(dur, 0.95);
      const firstP50 = pct(first, 0.5);
      return {
        name: ROLE_NAME[role] ?? human(role),
        calls: xs.length,
        errors: xs.filter((c) => c.error_kind !== undefined).length,
        ...(p50 === undefined ? {} : { p50 }),
        ...(p95 === undefined ? {} : { p95 }),
        ...(firstP50 === undefined ? {} : { firstP50 }),
        ...(costs.length ? { cost: costs.reduce((a, b) => a + b, 0) } : {}),
      };
    });
}

/** Step 2: calls and errors per stream minute, covering the whole session. */
export function perMinute(rs: ReviewSession, lengthS: number): { n: number; err: number }[] {
  const last = Math.max(lengthS, ...rs.calls.map((c) => callAt(rs, c)));
  const mins = Math.max(1, Math.ceil(last / 60));
  const out = Array.from({ length: mins }, () => ({ n: 0, err: 0 }));
  for (const c of rs.calls) {
    const m = out[Math.min(mins - 1, Math.floor(callAt(rs, c) / 60))];
    if (!m) continue;
    m.n++;
    if (c.error_kind !== undefined) m.err++;
  }
  return out;
}

export function EngineCalls(props: TabProps) {
  const { rs, d } = props;
  const [sel, setSel] = useState<{ minute: number; page: number } | null>(null);
  if (rs.calls.length === 0) {
    return (
      <div data-role="review-calls" data-rise>
        <EmptyState title="No engine calls" hint="No engine calls were recorded for this session." />
      </div>
    );
  }
  const roles = roleRows(rs.calls);
  const mins = perMinute(rs, d.lengthS);
  const maxN = Math.max(1, ...mins.map((m) => m.n));
  const rows = sel ? rs.calls.filter((c) => Math.min(mins.length - 1, Math.floor(callAt(rs, c) / 60)) === sel.minute).sort((a, b) => a.ts_ms - b.ts_ms) : [];
  return (
    <div data-role="review-calls" data-rise>
      <Section title="Step 1 · By role" right={`${rs.calls.length} engine calls`}>
        <div className={kit.scroll}>
          <table className={kit.table} data-role="review-calls-roles">
            <thead>
              <tr>
                <th>Role</th>
                <th className={kit.num}>Calls</th>
                <th className={kit.num}>Errors</th>
                <th className={kit.num}>Typical time</th>
                <th className={kit.num}>Slow (p95)</th>
                <th className={kit.num}>First words</th>
                <th className={kit.num}>Cost, USD</th>
              </tr>
            </thead>
            <tbody>
              {roles.map((r) => (
                <tr key={r.name}>
                  <td>{r.name}</td>
                  <td className={kit.num}>{r.calls}</td>
                  <td className={`${kit.num} ${r.errors ? kit.bad : ""}`}>{r.errors}</td>
                  <td className={kit.num}>{secs(r.p50)}</td>
                  <td className={kit.num}>{secs(r.p95)}</td>
                  <td className={kit.num}>{secs(r.firstP50)}</td>
                  <td className={kit.num}>{r.cost === undefined ? "–" : r.cost.toFixed(2)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Section>
      <Section title="Step 2 · Calls per minute" right="Pick a minute to see its calls">
        <div className={styles.minutes} data-role="review-calls-minutes">
          {mins.map((m, i) => {
            const label = `${fmtClock(i * 60)}: ${m.n} calls${m.err ? `, ${m.err} errors` : ""}`;
            return (
              <button key={i} type="button" title={label} aria-label={label} aria-pressed={sel?.minute === i} data-minute={i} onClick={() => setSel({ minute: i, page: 0 })}>
                <i style={{ height: `${(100 * m.n) / maxN}%` }} />
                {m.err > 0 && <i className={styles.err} style={{ height: `${(100 * m.err) / maxN}%` }} />}
              </button>
            );
          })}
        </div>
        <div className={kit.axis}>
          <span>00:00</span>
          <span>{fmtClock(mins.length * 60)}</span>
        </div>
      </Section>
      <Section title="Step 3 · Calls" right={sel ? `Minute ${fmtClock(sel.minute * 60)} · ${rows.length} calls` : undefined}>
        {!sel ? (
          <p className={kit.note}>The full list is never shown at once. Pick a minute above.</p>
        ) : (
          <div key={sel.minute} className={styles.minute}>
            <div className={kit.scroll}>
              <table className={kit.table} data-role="review-calls-rows">
                <thead>
                  <tr>
                    <th>At</th>
                    <th>Role</th>
                    <th>Model</th>
                    <th className={kit.num}>Time</th>
                    <th className={kit.num}>First words</th>
                    <th className={kit.num}>Cost</th>
                    <th>Problem</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.slice(sel.page * PAGE, (sel.page + 1) * PAGE).map((c, i) => (
                    <tr key={`${c.ts_ms}-${i}`}>
                      <td className={kit.mono}>{fmtClock(callAt(rs, c))}</td>
                      <td>{ROLE_NAME[c.role] ?? human(c.role)}</td>
                      <td>{c.model ?? (c.tier ? TIER_NAME[c.tier] : "–")}</td>
                      <td className={kit.num}>{secs(c.duration_ms)}</td>
                      <td className={kit.num}>{secs(c.first_text_ms)}</td>
                      <td className={kit.num}>{c.cost_usd === undefined ? "–" : c.cost_usd.toFixed(4)}</td>
                      <td className={c.error_kind ? kit.bad : undefined} title={c.fallback_reason}>
                        {c.error_kind ? human(c.error_kind) : c.fallback_reason ? "Fell back to another route" : ""}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <Pager page={sel.page} total={rows.length} onPage={(page) => setSel({ minute: sel.minute, page })} label="Calls" />
          </div>
        )}
      </Section>
    </div>
  );
}
