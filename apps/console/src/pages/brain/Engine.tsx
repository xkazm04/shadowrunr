// brain/engine: the routing the ledger shows, fleet accounts, fallbacks, and the virtualised
// call ledger with a role filter. Source: one tournament's ledgers (default: the newest), or the
// daemon's own engine ledger.
import { useMemo, useState } from "react";
import type { LedgerEntry, TournamentsPage } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { useRpc } from "../../data";
import { Chip, FilterTable, Section, StatBand, StatePanel, type Column, type Facets } from "../../ui";
import { latestTournament } from "./useBrain";
import { usePagedRpc } from "./queries";
import { summariseLedger, type EngineSummary } from "./engineModel";
import { clock, int, msText, usd } from "./events";
import { roleColor } from "./palette";
import styles from "./Layer2.module.css";

const DAEMON = "__daemon__";
const PAGE = 500;
const STEP = 10;

interface Row {
  readonly i: number;
  readonly l: LedgerEntry;
}

function at(l: LedgerEntry, s: EngineSummary): string {
  const t0 = l.run_id === undefined ? undefined : s.runStart.get(l.run_id);
  return t0 === undefined ? new Date(l.ts_ms).toLocaleTimeString("en-GB") : clock(l.ts_ms - t0);
}

function columns(s: EngineSummary): Column<Row>[] {
  return [
    { id: "run", header: "Run", cell: (r) => r.l.run_id ?? "daemon", sortValue: (r) => r.l.run_id ?? "" },
    { id: "at", header: "At", align: "end", cell: (r) => at(r.l, s), sortValue: (r) => r.l.ts_ms },
    {
      id: "role",
      header: "Role",
      cell: (r) => (
        <span className={styles.role}>
          <i style={{ background: roleColor(r.l.role) }} />
          {r.l.role}
        </span>
      ),
      sortValue: (r) => r.l.role,
    },
    { id: "tier", header: "Tier", cell: (r) => r.l.tier ?? "–" },
    { id: "acct", header: "Engine · account", cell: (r) => (r.l.engine ? `${r.l.engine} · ${r.l.account ?? "?"}` : "–") },
    { id: "ft", header: "First text", align: "end", cell: (r) => msText(r.l.first_text_ms), sortValue: (r) => r.l.first_text_ms ?? -1 },
    { id: "dur", header: "Duration", align: "end", cell: (r) => msText(r.l.duration_ms), sortValue: (r) => r.l.duration_ms },
    { id: "out", header: "Out tok", align: "end", cell: (r) => int(r.l.usage.output_tokens), sortValue: (r) => r.l.usage.output_tokens },
    { id: "cost", header: "Cost", align: "end", cell: (r) => usd(r.l.cost_usd ?? 0), sortValue: (r) => r.l.cost_usd ?? 0 },
    {
      id: "note",
      header: "Note",
      cell: (r) =>
        r.l.error_kind !== undefined ? <Chip tone="fail">{r.l.error_kind}</Chip> : r.l.fallback_reason !== undefined ? <Chip tone="caution">{r.l.fallback_reason}</Chip> : r.l.warm ? "warm" : "",
    },
  ];
}

const haystack = (r: Row) => `${r.l.run_id ?? ""} ${r.l.role} ${r.l.tier ?? ""} ${r.l.engine ?? ""} ${r.l.account ?? ""} ${r.l.model ?? ""} ${r.l.error_kind ?? ""} ${r.l.fallback_reason ?? ""}`;

function Tables({ rows, more, onMore }: { rows: readonly LedgerEntry[]; more: boolean; onMore: () => void }) {
  const s = useMemo(() => summariseLedger(rows), [rows]);
  const indexed = useMemo(() => rows.map((l, i) => ({ i, l })), [rows]);
  const fallbacks = useMemo(() => indexed.filter((r) => r.l.fallback_reason !== undefined), [indexed]);
  const cols = useMemo(() => columns(s), [s]);
  const facets = useMemo<Facets<Row, string>>(
    () => ({
      label: "Filter by role",
      initial: "all",
      options: [{ id: "all", label: "all", count: rows.length }, ...s.routes.map((r) => ({ id: r.role, label: r.role.replace("_", " "), count: r.calls }))],
      predicate: (r, f) => f === "all" || r.l.role === f,
    }),
    [s, rows.length],
  );
  return (
    <>
      <StatBand
        stats={[
          { label: more ? "Engine calls (first pages)" : "Engine calls", value: int(s.calls) },
          { label: "Errors", value: int(s.errors), ...(s.errors > 0 ? { tone: "fail" as const } : {}) },
          { label: "Fallbacks", value: int(s.fallbacks) },
          { label: "Cost", value: usd(s.cost) },
        ]}
      />
      <Section title="Routing" right="as the ledger shows it">
        <p className={styles.note}>The configured route table and live account cooldowns need the daemon&apos;s proposed `engine.status` call; until then this is what the engine did.</p>
        <table className={styles.table} data-role="brain-routes">
          <thead>
            <tr>
              <th>Role</th>
              <th>Tier</th>
              <th>Effort</th>
              <th>Engine</th>
              <th>Models</th>
              <th className={styles.num}>Calls</th>
              <th className={styles.num}>Errors</th>
              <th className={styles.num}>p50</th>
              <th className={styles.num}>p95</th>
              <th className={styles.num}>First text p50</th>
            </tr>
          </thead>
          <tbody>
            {s.routes.map((r) => (
              <tr key={r.role}>
                <td>
                  <span className={styles.role}>
                    <i style={{ background: roleColor(r.role) }} />
                    {r.role}
                  </span>
                </td>
                <td>{r.tiers.join(", ") || "–"}</td>
                <td>{r.efforts.join(", ") || "–"}</td>
                <td>{r.engines.join(", ") || "–"}</td>
                <td className={styles.mono}>{r.models.join(", ") || "–"}</td>
                <td className={styles.num}>{int(r.calls)}</td>
                <td className={styles.num}>{r.errors}</td>
                <td className={styles.num}>{msText(r.durationP50)}</td>
                <td className={styles.num}>{msText(r.durationP95)}</td>
                <td className={styles.num}>{msText(r.firstTextP50)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Section>
      <Section title="Fleet accounts" right={`${s.accounts.length} seen`}>
        <table className={styles.table} data-role="brain-accounts">
          <thead>
            <tr>
              <th>Engine</th>
              <th>Account</th>
              <th className={styles.num}>Calls</th>
              <th className={styles.num}>Errors</th>
              <th>Last call</th>
              <th>Models</th>
            </tr>
          </thead>
          <tbody>
            {s.accounts.map((a) => (
              <tr key={a.key}>
                <td className={styles.mono}>{a.engine}</td>
                <td>{a.account}</td>
                <td className={styles.num}>{int(a.calls)}</td>
                <td className={styles.num}>{a.errors}</td>
                <td>{a.lastError === undefined ? <Chip tone="ok">ok</Chip> : <Chip tone="fail">{a.lastError}</Chip>}</td>
                <td className={styles.mono}>{a.models.map((m) => `${m.model} ${m.calls}`).join(" · ")}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {s.unrouted > 0 && <p className={styles.note}>{int(s.unrouted)} ledger rows carry no engine or account: they errored before routing.</p>}
      </Section>
      <Section title="Fallbacks" right={int(fallbacks.length)}>
        {fallbacks.length === 0 ? (
          <p className={styles.note}>No call in these rows fell back to another engine.</p>
        ) : (
          <FilterTable rows={fallbacks} columns={cols} getKey={(r) => String(r.i)} haystack={haystack} label="Fallbacks" />
        )}
      </Section>
      <Section title="Call ledger" right={more ? <button type="button" className={styles.more} onClick={onMore}>Load {STEP * PAGE} more</button> : `${int(rows.length)} rows`}>
        <FilterTable rows={indexed} columns={cols} getKey={(r) => String(r.i)} haystack={haystack} label="Call ledger" facets={facets} placeholder="Filter by run, role, engine, model or error" />
      </Section>
    </>
  );
}

export function Engine() {
  const tournaments = useRpc("tournaments.page", { limit: 500 });
  const [picked, setPicked] = useState<string | null>(null);
  const [pages, setPages] = useState(STEP);
  const latest = tournaments.data ? latestTournament(tournaments.data.items) : undefined;
  const source = picked ?? latest ?? (tournaments.data ? DAEMON : null);
  const params = source === null ? null : source === DAEMON ? { limit: PAGE } : { tournament_id: source, limit: PAGE };
  const ledger = usePagedRpc("ledger.page", params, pages);
  return (
    <QueryView<TournamentsPage> query={tournaments}>
      {(t) => (
        <>
          <label className={styles.source}>
            Ledger
            <select
              value={source ?? DAEMON}
              onChange={(e) => {
                setPicked(e.target.value);
                setPages(STEP);
              }}
            >
              {t.items.map((x) => (
                <option key={x.tournament_id} value={x.tournament_id}>
                  {x.tournament_id} · {x.runs} runs
                </option>
              ))}
              <option value={DAEMON}>The daemon&apos;s engine ledger</option>
            </select>
          </label>
          {ledger.data.pages === 0 ? (
            ledger.errorView ? (
              <StatePanel state={ledger.errorView.state} title={ledger.errorView.title} hint={ledger.errorView.hint} action={{ label: "Retry", onClick: ledger.reload }} />
            ) : (
              <StatePanel state="loading" title="Reading the ledger" />
            )
          ) : ledger.data.items.length === 0 ? (
            <StatePanel state="empty" title="This ledger has no calls yet" />
          ) : (
            <Tables rows={ledger.data.items} more={ledger.data.more} onMore={() => setPages((p) => p + STEP)} />
          )}
        </>
      )}
    </QueryView>
  );
}
