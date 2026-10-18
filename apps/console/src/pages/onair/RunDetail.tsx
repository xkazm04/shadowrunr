// One run in the rundown: its segment spans as the winner's timeline bar, its counts, its beats
// (virtualised: a 100k-line transcript can hold thousands) and a button to replay it on screen.
import type { StreamEvent } from "@shadowrunr/contract";
import { segmentSpans } from "@shadowrunr/stage-kit";
import { useMemo } from "react";
import { useRpc } from "../../data";
import { QueryView } from "../../kit";
import { RouteLink, pageRoute, useRouter } from "../../router";
import { Section, StatBand, VirtualList } from "../../ui";
import { runSource, selectSource } from "./feed";
import styles from "./Layer2.module.css";
import { beatsOf, clock, countsOf } from "./beats";

function Anatomy({ events, tournamentId, runId }: { events: readonly StreamEvent[]; tournamentId: string; runId: string }) {
  const router = useRouter();
  const spans = useMemo(() => segmentSpans(events), [events]);
  const beats = useMemo(() => beatsOf(events), [events]);
  const n = useMemo(() => countsOf(events), [events]);
  const total = Math.max(1, n.end - n.start);
  return (
    <>
      <StatBand
        stats={[
          { label: "Length", value: clock(n.end - n.start) },
          { label: "Events", value: n.events.toLocaleString("en-US") },
          { label: "Lines spoken", value: n.spoken.toLocaleString("en-US") },
          { label: "Chat lines", value: n.chat.toLocaleString("en-US") },
          { label: "Picked", value: n.picked.toLocaleString("en-US") },
        ]}
      />
      <div className={styles.tlBar} role="img" aria-label={`Segments: ${spans.map((s) => `${s.segment} ${clock(s.from - n.start)} to ${clock(s.to - n.start)}`).join(", ")}`} data-role="onair-run-spans">
        {spans.map((s) => (
          <i key={`${s.segment}@${s.from}`} data-seg={s.segment} style={{ flexGrow: Math.max(1, s.to - s.from) / total }} title={`${s.segment} ${clock(s.from - n.start)} to ${clock(s.to - n.start)}`}>
            <b>{s.segment.replace("SEG-", "")}</b>
          </i>
        ))}
      </div>
      <div className={styles.actions}>
        <button
          type="button"
          className={styles.primary}
          onClick={() => {
            selectSource(runSource({ run_id: runId, tournament_id: tournamentId }));
            router.navigate(pageRoute("onair"));
          }}
        >
          Replay on screen
        </button>
        <RouteLink to={pageRoute("brain", "transcripts", runId)} className={styles.more}>
          Read the transcript
        </RouteLink>
      </div>
      <h3 className={styles.sub}>Beats · {beats.length.toLocaleString("en-US")}</h3>
      {beats.length === 0 ? (
        <p className={styles.note}>No beats: the core thread called no ops in this run.</p>
      ) : (
        <VirtualList
          className={styles.beats}
          label="Beats"
          count={beats.length}
          estimateRem={2}
          getKey={(i) => beats[i]?.line ?? i}
          renderRow={(i) => {
            const b = beats[i];
            if (!b) return null;
            return (
              <div className={styles.beat} data-op={b.op || undefined}>
                <span className={styles.num}>{clock(b.ts - n.start)}</span>
                <b>{b.kind}</b>
                <span className={styles.detail}>{b.detail}</span>
                <RouteLink to={pageRoute("brain", "transcripts", runId, String(b.line))} className={styles.ref}>
                  L{b.line}
                </RouteLink>
              </div>
            );
          }}
        />
      )}
    </>
  );
}

export function RunDetail({ tournamentId, runId }: { tournamentId: string; runId: string }) {
  const t = useRpc("run.transcript", { run_id: runId, tournament_id: tournamentId });
  return (
    <Section title={`Run ${runId}`} right={tournamentId}>
      <QueryView query={t} isEmpty={(d) => d.length === 0} emptyTitle="This run has no events" emptyHint="It was recorded empty; pick another run below.">
        {(events) => <Anatomy events={events} tournamentId={tournamentId} runId={runId} />}
      </QueryView>
    </Section>
  );
}
