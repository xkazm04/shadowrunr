// segments/ranks[/<report id>]: the ladder (thresholds, weights, titles), the vote calculator,
// and engagement by rank from a reconcile report (l2b.js `ranks`; A/1 FLOORS.segments ranks).
import type { RankInfo, RankTableInfo } from "@shadowrunr/contract";
import { useState, type CSSProperties, type FormEvent } from "react";
import { QueryView } from "../../kit";
import { RankEngagement } from "../../kit/RankEngagement";
import { useNewestReport } from "../../kit/useNewestReport";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { Button } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import { Head, Section } from "../../ui/Section";
import { StatePanel } from "../../ui/States";
import { useRanks, useReport } from "./hooks";
import styles from "./Layer2.module.css";
import { fmtWeight, parseVotes, qualifies, RANK_KEY, rankIndex, START_VOTES, tally, type Votes } from "./model";

export function RanksPage({ reportId }: { reportId: string | undefined }) {
  const ranks = useRanks();
  return (
    <>
      <Head
        kicker="Segments & ranks · layer 2"
        title="Ranks and weights"
        lede="A viewer holds the highest rank any of their conditions meets. Weights grow sub-linearly, so a Dread Council vote never drowns out the room."
      />
      <p className={styles.banner} data-role="segments-assumptions">
        Assumptions, not measured. Every threshold and weight here is a starting proposal. None is calibrated against real audience data: recalibrate after the first Season 0 streams.
      </p>
      <QueryView query={ranks} isEmpty={(t) => t.ranks.length === 0} emptyTitle="The rank table is empty">
        {(t) => (
          <>
            <Ladder table={t} />
            <Calculator ranks={t.ranks} />
            <Engagement table={t} reportId={reportId} />
          </>
        )}
      </QueryView>
    </>
  );
}

const rankClass = (r: RankInfo["rank"]) => styles[RANK_KEY[r]] ?? "";

function Ladder({ table }: { table: RankTableInfo }) {
  const max = Math.max(...table.ranks.map((r) => r.weight), 1);
  const gate = rankIndex(table.propose_min_rank);
  return (
    <Section title="The ladder" right={`${table.character} · ${table.ranks.length} ranks`}>
      <div className={styles.tblwrap}>
        <table className={styles.tbl} data-role="segments-ladder">
          <thead>
            <tr>
              <th>Rank</th>
              <th className={styles.num}>Vote weight</th>
              <th>Gifted subs</th>
              <th>Subscription</th>
              <th>!propose</th>
            </tr>
          </thead>
          <tbody>
            {table.ranks
              .slice()
              .reverse()
              .map((r) => {
                const q = qualifies(r);
                return (
                  <tr key={r.rank} data-role="segments-ladder-row" data-rank={r.rank}>
                    <td>
                      <span className={`${styles.rkDot} ${rankClass(r.rank)}`} aria-hidden="true" />
                      <b>{r.title}</b>
                      <small>
                        <code>{r.rank}</code>
                      </small>
                    </td>
                    <td className={styles.num}>
                      <span className={styles.wbar} aria-hidden="true">
                        <i style={{ width: `${((r.weight / max) * 100).toFixed(0)}%` } as CSSProperties} />
                      </span>
                      {fmtWeight(r.weight)}
                    </td>
                    <td>{q.gifted}</td>
                    <td>{q.tiers || <span className={styles.dim}>base rank</span>}</td>
                    <td>{rankIndex(r.rank) >= gate ? <Chip tone="ok">yes</Chip> : <Chip tone="muted">vote only</Chip>}</td>
                  </tr>
                );
              })}
          </tbody>
        </table>
      </div>
    </Section>
  );
}

function Calculator({ ranks }: { ranks: readonly RankInfo[] }) {
  const [votes, setVotes] = useState<Votes>(START_VOTES);
  const t = tally(ranks, votes);
  const set = (rank: RankInfo["rank"], side: "a" | "b", value: string) => setVotes((v) => ({ ...v, [rank]: { ...v[rank], [side]: parseVotes(value) } }));
  const w = (n: number) => ({ width: `${((n / t.max) * 100).toFixed(0)}%` }) as CSSProperties;
  return (
    <Section title="Try a vote">
      <p className={styles.prose}>Set how many voters of each rank back each option, and see the head-count and the weight disagree. Illustrative arithmetic with the assumed weights; nothing is sent.</p>
      <div className={styles.calc} data-role="segments-calc">
        <div className={styles.calcH}>
          <span />
          <b>Option A</b>
          <b>Option B</b>
        </div>
        {ranks.map((r) => (
          <div key={r.rank} className={styles.calcR}>
            <span>
              <i className={`${styles.rkDot} ${rankClass(r.rank)}`} aria-hidden="true" />
              {r.title} <small>{fmtWeight(r.weight)}</small>
            </span>
            {(["a", "b"] as const).map((side) => (
              <input
                key={side}
                type="number"
                min={0}
                max={999}
                value={votes[r.rank][side]}
                aria-label={`${r.title} voters for ${side.toUpperCase()}`}
                onChange={(e) => set(r.rank, side, e.target.value)}
              />
            ))}
          </div>
        ))}
      </div>
      <div className={styles.calcOut} data-role="segments-calc-out" aria-live="polite">
        {(["a", "b"] as const).map((side) => (
          <div key={side} className={styles.co}>
            <b>Option {side.toUpperCase()}</b>
            <span>head-count {t.heads[side]}</span>
            <i className={`${styles.cb} ${styles.cbH}`} style={w(t.heads[side])} />
            <span>weight {t.weight[side].toFixed(1)}</span>
            <i className={`${styles.cb} ${styles.cbW}`} style={w(t.weight[side])} />
          </div>
        ))}
        <p className={styles.dim} data-role="segments-calc-verdict">
          {t.verdict}
        </p>
      </div>
    </Section>
  );
}

/** The report in the route, else the newest the daemon stores (`reconcile.list`). */
function Engagement({ table, reportId }: { table: RankTableInfo; reportId: string | undefined }) {
  const shown = useNewestReport(reportId);
  const id = shown.reportId;
  const right = id === undefined ? "no report yet" : shown.newest ? `${id} · newest` : id;
  return (
    <Section title="Engagement by rank" right={right}>
      {id !== undefined ? (
        <ReportEngagement table={table} reportId={id} />
      ) : shown.pending ? (
        <StatePanel state="loading" title="Finding the newest report" />
      ) : (
        <ChooseReport />
      )}
    </Section>
  );
}

/** No report stored yet (or the list failed): run one in Reconcile, or name an id. */
function ChooseReport() {
  const router = useRouter();
  const [id, setId] = useState("");
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const v = id.trim();
    if (v) router.navigate(pageRoute("segments", "ranks", v));
  };
  return (
    <div data-role="segments-engagement-empty">
      <p className={styles.note}>
        Who chatted, who got picked and who got answered, by rank, comes from a reconcile report. Open one from{" "}
        <RouteLink to={pageRoute("reconcile", "report")} className={styles.link}>
          Reconcile
        </RouteLink>
        , or name its id here.
      </p>
      <form className={styles.form} onSubmit={submit}>
        <label className={styles.fld}>
          <span>Report id</span>
          <input type="text" value={id} placeholder="rec-1792284840627" onChange={(e) => setId(e.target.value)} />
        </label>
        <Button type="submit" variant="ink" disabled={id.trim() === ""}>
          Show engagement
        </Button>
      </form>
    </div>
  );
}

function ReportEngagement({ table, reportId }: { table: RankTableInfo; reportId: string }) {
  const report = useReport(reportId);
  const title = (rank: string) => table.ranks.find((r) => r.rank === rank)?.title ?? rank;
  return (
    <QueryView query={report} isEmpty={(r) => (r.ranks.by_rank ?? []).length === 0} emptyTitle="No ranked chat in this report" emptyHint="Ranks with no messages are left out of the report.">
      {(r) => (
        <>
          <RankEngagement rows={r.ranks.by_rank ?? []} title={title} role="segments-engagement" />
          <p className={styles.note}>Viewers are simulated in the trials. Rates carry their n and bound; ranks with no messages are left out.</p>
        </>
      )}
    </QueryView>
  );
}
