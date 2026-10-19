// `#/onair/clips`: the clip factory's gate, graphic first. The funnel of candidates by publish
// state as poster numerals (from `clips.list` counts over the whole store), then the
// candidates best first, each a link to its nested page. Scores always carry "weights
// unmeasured": the blend is a design hypothesis until an editor's picks score it.
import type { ClipSummary, ClipsPage } from "@shadowrunr/contract";
import { useState } from "react";
import { useRpc } from "../../data";
import { QueryView } from "../../kit";
import { pageRoute, RouteLink } from "../../router";
import { Chip } from "../../ui/Chip";
import { StatBand } from "../../ui/StatBand";
import { STATUS_LABEL, STATUS_ORDER, STATUS_TONE, WEIGHTS_LABEL, score } from "./clipModel";
import styles from "./Clips.module.css";

const PAGE = 50;

export function Clips() {
  // Cursors of the pages before this one; `undefined` is the first page.
  const [cursors, setCursors] = useState<(string | undefined)[]>([undefined]);
  const cursor = cursors[cursors.length - 1];
  const list = useRpc("clips.list", cursor === undefined ? { limit: PAGE } : { limit: PAGE, cursor });
  return (
    <div data-role="onair-clips">
      <QueryView
        query={list}
        isEmpty={(p) => p.total === 0 && Object.values(p.counts).every((n) => !n)}
        emptyTitle="No clip candidates yet"
        emptyHint="Scan a run or a tournament: shadowrunr clips scan --tournament <id>."
      >
        {(p) => (
          <>
            <Funnel page={p} />
            <ul className={styles.list} data-role="onair-clips-list">
              {p.items.map((c, i) => (
                <Row key={c.id} c={c} n={PAGE * (cursors.length - 1) + i + 1} />
              ))}
            </ul>
            <div className={styles.pager}>
              <button type="button" className={styles.secondary} disabled={cursors.length === 1} onClick={() => setCursors((c) => c.slice(0, -1))}>
                Previous
              </button>
              <span className={styles.dim}>
                {p.items.length} of {p.total}
              </span>
              <button
                type="button"
                className={styles.secondary}
                disabled={p.next_cursor === undefined}
                onClick={() => p.next_cursor !== undefined && setCursors((c) => [...c, p.next_cursor])}
              >
                Next
              </button>
            </div>
          </>
        )}
      </QueryView>
    </div>
  );
}

function Funnel({ page }: { page: ClipsPage }) {
  const held = page.held;
  return (
    <div data-role="onair-clips-funnel">
      <StatBand
        stats={[
          ...STATUS_ORDER.map((s) => ({ label: STATUS_LABEL[s], value: page.counts[s] ?? 0, ...(s === "rejected" && (page.counts[s] ?? 0) > 0 ? { tone: "fail" as const } : {}) })),
          { label: "Held at the gate", value: held, ...(held > 0 ? { tone: "fail" as const } : {}) },
        ]}
      />
      <p className={styles.note}>
        Score = 0.35·marker + 0.25·chat + 0.15·audio + 0.25·hook, {WEIGHTS_LABEL}; audio is unmeasured in the media-less phase and its share is
        redistributed. Nothing publishes without the owner’s signature, and uploads are dry runs.
      </p>
    </div>
  );
}

function Row({ c, n }: { c: ClipSummary; n: number }) {
  return (
    <li>
      <RouteLink to={pageRoute("onair", "clips", c.id)} className={styles.row} data-role="onair-clip-row">
        <span className={styles.rank}>{n}</span>
        <b className={styles.score} title={`${WEIGHTS_LABEL}; rank ${c.rank} in its run`}>
          {score(c.score)}
        </b>
        <span className={styles.title}>{c.title}</span>
        <span className={styles.chips}>
          <Chip tone={STATUS_TONE[c.status]}>{STATUS_LABEL[c.status]}</Chip>
          <Chip tone={c.lane === "clip" ? "ok" : "muted"} title={c.lane === "clip" ? "two or more signals agree" : "one signal carries it: review, not queue"}>
            {c.lane}
          </Chip>
          {c.held && <Chip tone="fail">held</Chip>}
          {c.requires_mandatory_gate && <Chip tone="caution" title="SEG-02A or a linked case: the evidence checklist applies">alleged</Chip>}
          <Chip tone="muted">{c.render ?? "no render"}</Chip>
        </span>
        <span className={styles.meta}>
          {c.segment} · {c.duration_s.toFixed(1)} s · {c.run_id}
        </span>
      </RouteLink>
    </li>
  );
}
