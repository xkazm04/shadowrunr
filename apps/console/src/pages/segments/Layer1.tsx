// Layer 1: the winner's programme poster (l1.js P3.L1.segments, l1.css .sg-*): lifecycle
// columns, chat commands and the rank pyramid, fused with A/1's lifecycle track (its descoped
// siding) and the ziggurat's !propose ledge (A/1 app.js SCENES.segments). Each block is a lit
// link into its layer-2 page.
import type { CSSProperties, ReactNode } from "react";
import type { RankTableInfo, SegmentTables } from "@shadowrunr/contract";
import { useConnection, type Query } from "../../data";
import { PosterFrame } from "../../kit/PosterFrame";
import { pageRoute, RouteLink } from "../../router";
import { useRanks, useTables } from "./hooks";
import styles from "./Layer1.module.css";
import { fmtWeight, lifecycleColumns, pyramid, RANK_KEY, type Column } from "./model";
import { Track } from "./Track";

export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <ProgrammePoster />;
}

const COLUMN_CLASS: Readonly<Record<Column["status"], string | undefined>> = {
  pilot: styles.cPilot,
  trial: styles.cTrial,
  regular: styles.cRegular,
  flagship: styles.cFlagship,
  retired: styles.cRetired,
};

function ProgrammePoster() {
  const tables = useTables();
  const ranks = useRanks();
  const slip = useSlip(tables, ranks);
  const segs = tables.data?.segments;

  return (
    <PosterFrame
      className={styles.root}
      role="segments-layer1"
      measure
      credit="The programme of the night · "
      cta={{ to: pageRoute("segments", "tables"), label: "Open the segment tables", role: "segments-cta" }}
    >
      <div className={styles.hero} data-role="segments-hero">
        <RouteLink
          to={pageRoute("segments", "tables")}
          className={`${styles.hot} ${styles.life}`}
          title="Pilot, trial, regular, flagship, retired: the segment tables"
          aria-label="Segment lifecycle: open the segment tables"
          data-role="segments-lifecycle"
        >
          <div className={styles.h}>Lifecycle</div>
          {lifecycleColumns(segs ?? []).map((c) => (
            <LifeColumn key={c.status} column={c} known={segs !== undefined} />
          ))}
          {segs && <Track segments={segs} />}
        </RouteLink>
        <RouteLink
          to={pageRoute("segments", "commands")}
          className={`${styles.hot} ${styles.cmd}`}
          title="The chat commands, who may use them and where"
          aria-label="Chat commands: open the commands and the op grid"
          data-role="segments-commands"
        >
          <div className={styles.h}>Chat commands</div>
          {tables.data?.command_names.map((c) => (
            <code key={c} data-role="segments-command">
              !{c}
            </code>
          ))}
        </RouteLink>
        <RouteLink
          to={pageRoute("segments", "ranks")}
          className={`${styles.hot} ${styles.pyr}`}
          title="Rank thresholds and vote weights (assumptions, not measured)"
          aria-label="The rank pyramid: open ranks and weights"
          data-role="segments-pyramid"
        >
          <div className={styles.h}>
            Rank pyramid <em>vote weight · an assumption, not measured</em>
          </div>
          {ranks.data && <Pyramid table={ranks.data} />}
        </RouteLink>
        {slip && (
          <p className={styles.slip} role="status" data-role="segments-state">
            {slip.text}
            {slip.action}
          </p>
        )}
      </div>
    </PosterFrame>
  );
}

function LifeColumn({ column: c, known }: { column: Column; known: boolean }) {
  const empty = known && c.total === 0;
  return (
    <div className={`${styles.lc} ${COLUMN_CLASS[c.status] ?? ""}`} data-role="segments-lc" data-status={c.status} data-empty={empty}>
      <h3>{c.status}</h3>
      <div className={styles.tk}>
        {c.shown.map((s, i) => (
          <span key={`${s.id}:${i}`} className={c.status === "retired" ? `${styles.tick} ${styles.x}` : styles.tick} data-role="segments-tick">
            <b>{s.id}</b>
            {s.title}
          </span>
        ))}
        {c.more > 0 && (
          <span className={`${styles.tick} ${styles.more}`} data-role="segments-tick-more">
            <b>+{c.more.toLocaleString("en-US")}</b>more in the tables
          </span>
        )}
        {empty && <span className={styles.none}>none yet</span>}
      </div>
    </div>
  );
}

function Pyramid({ table }: { table: RankTableInfo }) {
  const steps = pyramid(table);
  return (
    <>
      {steps.map((s) => (
        <div
          key={s.rank}
          className={`${styles.st} ${styles[RANK_KEY[s.rank]] ?? ""} ${s.ledge ? styles.ledge : ""}`}
          style={{ "--w": `${s.widthPct}%` } as CSSProperties}
          data-role="segments-step"
          data-rank={s.rank}
          data-proposes={s.proposes}
        >
          <span className={styles.t}>{s.title}</span>
          <span className={styles.w}>{fmtWeight(s.weight)}</span>
          {s.ledge && (
            <span className={styles.ledgeTag} data-role="segments-propose-ledge">
              !propose from {s.title} up
            </span>
          )}
        </div>
      ))}
    </>
  );
}

type Slip = { readonly text: string; readonly action?: ReactNode } | undefined;

function useSlip(tables: Query<SegmentTables>, ranks: Query<RankTableInfo>): Slip {
  const conn = useConnection();
  const failed = tables.errorView ?? ranks.errorView;
  const retry = () => {
    tables.reload();
    ranks.reload();
  };
  if (tables.data && ranks.data) {
    return failed ? { text: `Showing the last good tables · ${failed.title}` } : undefined;
  }
  if (failed) {
    const text = failed.state === "offline" || conn.summary === "offline" ? "The daemon is not reachable. The programme shows here once it answers." : failed.title;
    return {
      text,
      action: (
        <button type="button" className={styles.slipAction} onClick={retry}>
          Retry
        </button>
      ),
    };
  }
  return { text: "Reading the segment tables…" };
}
