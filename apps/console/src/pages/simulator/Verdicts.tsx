// simulator/verdicts[/<run>]: the judged runs, then one judge verdict: rule checks grouped by
// kind (failures first) and quality facts with quote verification against the transcript
// (A/1 core.js:241-249): each quote is looked up on its cited line ±2.
import { useMemo } from "react";
import type { JudgeVerdict, QualityFact, RuleCheck, RunRow, StreamEvent } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { usePaged, useRpc, type Query } from "../../data";
import { pageRoute, useRouter } from "../../router";
import { Chip, State } from "../../ui/Chip";
import { FilterTable, type Column } from "../../ui/FilterTable";
import { Head, Section } from "../../ui/Section";
import { StatBand } from "../../ui/StatBand";
import { PAGE_LIMIT } from "./data";
import { parseRef } from "../../kit/events";
import { verifyQuote, type QuoteCheck } from "./quote";
import { int } from "../../kit/events";
import { Note, RefLink, RefList, Found } from "./common";
import styles from "./l2.module.css";

const pickColumns: readonly Column<RunRow>[] = [
  { id: "run", header: "Run", cell: (r) => <span className={styles.code}>{r.run_id}</span>, sortValue: (r) => r.run_id },
  { id: "t", header: "Tournament", cell: (r) => r.tournament_id, sortValue: (r) => r.tournament_id },
  { id: "tone", header: "Tone", cell: (r) => r.spec.tone_variant, sortValue: (r) => r.spec.tone_variant },
  { id: "tier", header: "Tier", cell: (r) => r.spec.core_tier, sortValue: (r) => r.spec.core_tier },
  { id: "aud", header: "Viewers", cell: (r) => int(r.spec.audience_size), sortValue: (r) => r.spec.audience_size, align: "end" },
];

export function VerdictPicker() {
  const router = useRouter();
  const paged = usePaged("runs.page", { limit: PAGE_LIMIT, filter: { has_verdict: true } });
  return (
    <>
      <Head
        kicker="Simulator · layer 2"
        title="Judge verdicts"
        lede="The judge reads the transcript and the rubric, never the constitution, the tone or the brief. Every quoted line is checked against the transcript."
      />
      <QueryView query={paged.query} isEmpty={(r) => r.length === 0} emptyTitle="No judged runs yet" emptyHint="Verdicts appear once a tournament runs with judging on.">
        {(rows) => (
          <FilterTable
            rows={rows}
            columns={pickColumns}
            getKey={(r) => `${r.tournament_id}/${r.run_id}`}
            haystack={(r) => `${r.run_id} ${r.tournament_id} ${r.spec.tone_variant} ${r.spec.core_tier}`}
            label="Judged runs"
            viewKey="simulator.verdicts"
            placeholder={`Filter ${int(rows.length)} judged runs${paged.done ? "" : " (loading more)"}`}
            onActivate={(r) => router.navigate(pageRoute("simulator", "verdicts", r.run_id))}
          />
        )}
      </QueryView>
    </>
  );
}

/** Failures first, then not evaluable, pass, not provoked (the winner's order). */
const ORDER: Readonly<Record<RuleCheck["state"], number>> = { fail: 0, not_evaluable: 1, pass: 2, not_provoked: 3 };

function groupRules(rules: readonly RuleCheck[]): [string, RuleCheck[]][] {
  const groups = new Map<string, RuleCheck[]>();
  for (const r of rules) {
    const kind = r.rule.includes(":") ? (r.rule.split(":")[0] ?? r.rule) : "other";
    const g = groups.get(kind) ?? [];
    g.push(r);
    groups.set(kind, g);
  }
  return [...groups.entries()]
    .map(([k, g]): [string, RuleCheck[]] => [k, [...g].sort((a, b) => ORDER[a.state] - ORDER[b.state] || a.rule.localeCompare(b.rule))])
    .sort((a, b) => Math.min(...a[1].map((r) => ORDER[r.state])) - Math.min(...b[1].map((r) => ORDER[r.state])) || a[0].localeCompare(b[0]));
}

const CHECK_LABEL: Readonly<Record<QuoteCheck["state"], string>> = { verified: "verified", nearby: "nearby", not_found: "not found" };
const CHECK_TONE = { verified: "ok", nearby: "caution", not_found: "fail" } as const;

function QuoteState({ fact, transcript }: { fact: QualityFact; transcript: Query<StreamEvent[]> }) {
  const ref = parseRef(fact.turn_ref);
  // A run-level fact (WP15: a try directive or the dispatch not attempted) cites the run, not a line.
  if (!ref && fact.quote === "") return <Chip tone="muted" title="Judged over the whole run: there is no line to quote.">whole run</Chip>;
  if (!ref) return <Chip tone="unevaluable">bad ref</Chip>;
  if (transcript.data === undefined) {
    return transcript.errorView ? <Chip tone="muted">transcript unavailable</Chip> : <Chip tone="muted">checking</Chip>;
  }
  const c = verifyQuote(transcript.data, ref.line, fact.quote);
  const where = c.state === "nearby" ? ` (L${c.line}, ${c.offset > 0 ? "+" : ""}${c.offset})` : "";
  return (
    <span data-check={c.state}>
      <Chip tone={CHECK_TONE[c.state]} {...(c.state === "not_found" ? { title: "The quoted words are not on the cited line or the two lines either side." } : {})}>
        {CHECK_LABEL[c.state]}
      </Chip>
      {where}
    </span>
  );
}

export function VerdictDetail({ runId }: { runId: string }) {
  const verdict = useRpc("run.verdict", { run_id: runId });
  const transcript = useRpc("run.transcript", { run_id: runId });
  return (
    <>
      <Head kicker="Simulator · judge verdict" title={runId} />
      <Found query={verdict} emptyTitle="This run has no verdict yet" emptyHint="Runs are judged when the tournament runs with judging on.">
        {(v) => <Verdict v={v} transcript={transcript} />}
      </Found>
    </>
  );
}

function Verdict({ v, transcript }: { v: JudgeVerdict; transcript: Query<StreamEvent[]> }) {
  const counts = useMemo(() => {
    const c: Record<RuleCheck["state"], number> = { pass: 0, fail: 0, not_provoked: 0, not_evaluable: 0 };
    for (const r of v.rules) c[r.state] += 1;
    return c;
  }, [v.rules]);
  const groups = useMemo(() => groupRules(v.rules), [v.rules]);
  const checks = useMemo(() => {
    const t = transcript.data;
    if (!t) return undefined;
    let verified = 0;
    for (const q of v.quality) {
      const r = parseRef(q.turn_ref);
      if (r && verifyQuote(t, r.line, q.quote).state === "verified") verified += 1;
    }
    return verified;
  }, [v.quality, transcript.data]);
  return (
    <>
      <StatBand
        stats={[
          { label: "pass", value: int(counts.pass) },
          { label: "fail", value: int(counts.fail), ...(counts.fail > 0 ? { tone: "fail" as const } : {}) },
          { label: "not provoked", value: int(counts.not_provoked) },
          { label: "not evaluable", value: int(counts.not_evaluable) },
          { label: `judge · ${v.judge_tier}`, value: checks === undefined ? "…" : `${checks}/${v.quality.length}` },
        ]}
      />
      <Note>
        The judge tier is never the tier it judges. The last figure counts quality quotes found verbatim on the line they cite.
      </Note>
      <Section title="Quality · verified quotes" right={`${v.quality.length} quotes`}>
        {v.quality.length === 0 ? (
          <p className={styles.dim}>The judge recorded no quality facts for this run.</p>
        ) : (
          <table className={styles.table} data-role="simulator-quotes">
            <thead>
              <tr>
                <th>Dimension</th>
                <th>Value</th>
                <th>Quote</th>
                <th>Check</th>
                <th>Line</th>
              </tr>
            </thead>
            <tbody>
              {v.quality.map((q, i) => (
                <tr key={`${q.dimension}-${i}`}>
                  <td>{q.dimension.replace(/_/g, " ")}</td>
                  <td>
                    <Chip tone={q.value ? "ok" : "fail"}>{q.value ? "true" : "false"}</Chip>
                  </td>
                  <td className={styles.quote}>“{q.quote}”</td>
                  <td>
                    <QuoteState fact={q} transcript={transcript} />
                  </td>
                  <td>
                    <RefLink refStr={q.turn_ref} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>
      {groups.map(([kind, rules]) => (
        <Section key={kind} title={kind.replace(/-/g, " ")} right={`${rules.length} rules`}>
          <table className={styles.table}>
            <tbody>
              {rules.map((r) => (
                <tr key={r.rule}>
                  <td className={styles.rule}>{r.rule.includes(":") ? r.rule.slice(r.rule.indexOf(":") + 1) : r.rule}</td>
                  <td>
                    <State value={r.state} />
                  </td>
                  <td>
                    <RefList refs={r.evidence ?? []} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Section>
      ))}
      {v.notes && (
        <Section title="The judge's notes">
          <blockquote className={styles.notes}>{v.notes}</blockquote>
        </Section>
      )}
    </>
  );
}
