// brain/transcripts/<run>[/<line>]: the virtualised transcript viewer. Type pills and a search
// filter the lines; a line is a deep link (`.../<line>`), scrolled into view and opened in the
// detail pane with its raw event. Only a screenful of rows is ever in the DOM (100k lines).
// "Open in stage" plays the run on the broadcast stage, outside the console.
import { useDeferredValue, useMemo, useState } from "react";
import type { RunTranscriptResult, StreamEvent } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { OpenInStage } from "../../kit/OpenInStage";
import { buttonClass } from "../../ui/Button";
import { useRpc } from "../../data";
import { pageRoute, useRouter } from "../../router";
import { PillGroup, type PillOption } from "../../ui/PillGroup";
import { VirtualList } from "../../ui/VirtualList";
import { clock, eventText, eventWho, int, isMemo } from "../../kit/events";
import styles from "./Viewer.module.css";

type TypeFilter = "all" | "say" | "chat" | "picked" | "op" | "vote" | "moderation" | "job";
const TYPES: readonly PillOption<TypeFilter>[] = (["all", "say", "chat", "picked", "op", "vote", "moderation", "job"] as const).map((id) => ({ id, label: id }));

/** Indices of the lines that pass the type and text filters. */
export function filterLines(events: readonly StreamEvent[], type: TypeFilter, query: string): number[] {
  const q = query.trim().toLowerCase();
  const out: number[] = [];
  for (let i = 0; i < events.length; i++) {
    const e = events[i];
    if (!e || (type !== "all" && e.type !== type)) continue;
    if (q && !`${eventWho(e)} ${eventText(e)}`.toLowerCase().includes(q)) continue;
    out.push(i);
  }
  return out;
}

const ROW_REM = 3.1;

function Lines(props: { events: readonly StreamEvent[]; shown: readonly number[]; selected: number | undefined; onPick: (i: number) => void }) {
  const { events, shown, selected, onPick } = props;
  const at = selected === undefined ? -1 : shown.indexOf(selected);
  return (
    <VirtualList
      className={styles.list}
      label="Transcript lines"
      role="brain-lines"
      count={shown.length}
      estimateRem={ROW_REM}
      getKey={(k) => shown[k] ?? k}
      scrollToIndex={at >= 0 ? at : undefined}
      renderRow={(k) => {
        const i = shown[k];
        const e = i === undefined ? undefined : events[i];
        if (i === undefined || !e) return null;
        return (
          <button type="button" className={styles.line} aria-current={i === selected ? "true" : undefined} onClick={() => onPick(i)} data-line={i + 1}>
            <span className={styles.n}>{i + 1}</span>
            <span className={styles.t}>{clock(e.ts_ms)}</span>
            <span className={styles.ty} data-type={e.type}>
              {e.type === "op" ? e.op.op.replace(/_/g, " ") : e.type}
            </span>
            <span className={styles.tx}>
              <b>{eventWho(e)}</b>
              {eventText(e)}
            </span>
          </button>
        );
      }}
    />
  );
}

function Detail({ events, index, run }: { events: readonly StreamEvent[]; index: number | undefined; run: string }) {
  const e = index === undefined ? undefined : events[index];
  if (index === undefined || !e) return <p className={styles.hint}>Select a line to read it in full, with its raw event.</p>;
  return (
    <div className={styles.lineDetail} data-role="brain-line-detail">
      <div className={styles.kicker}>
        Line {index + 1} · {run}#{index + 1}
      </div>
      <h3 className={styles.who}>
        {eventWho(e)} · {e.type} · {clock(e.ts_ms)}
      </h3>
      <p className={e.type === "say" ? styles.voice : styles.body}>{eventText(e)}</p>
      {isMemo(e) && <p className={styles.hint}>Memory candidate: not spoken on air.</p>}
      <pre className={styles.raw}>{JSON.stringify(e, null, 2)}</pre>
    </div>
  );
}

function Transcript({ run, line, events }: { run: string; line: number | undefined; events: RunTranscriptResult }) {
  const router = useRouter();
  const [type, setType] = useState<TypeFilter>("all");
  const [query, setQuery] = useState("");
  const deferred = useDeferredValue(query);
  const shown = useMemo(() => filterLines(events, type, deferred), [events, type, deferred]);
  const selected = line !== undefined && line <= events.length ? line - 1 : undefined;
  const pick = (i: number) => router.navigate(pageRoute("brain", "transcripts", run, String(i + 1)), { replace: true });
  return (
    <>
      <div className={styles.tools}>
        <PillGroup label="Filter by event type" options={TYPES} value={type} onChange={setType} />
        <input type="search" className={styles.search} value={query} placeholder="Search text or name" aria-label="Search the transcript" onChange={(e) => setQuery(e.target.value)} />
        <span className={styles.count} aria-live="polite">
          {int(shown.length)} of {int(events.length)} lines
        </span>
        <OpenInStage runId={run} className={buttonClass("ink")} role="brain-open-stage" />
      </div>
      {line !== undefined && selected === undefined && <p className={styles.hint}>This transcript has no line {line}; it ends at line {events.length}.</p>}
      <div className={styles.viewer}>
        <Lines events={events} shown={shown} selected={selected} onPick={pick} />
        <div className={styles.detail}>
          <Detail key={selected ?? "none"} events={events} index={selected} run={run} />
        </div>
      </div>
    </>
  );
}

export function Viewer({ run, line }: { run: string; line: number | undefined }) {
  const transcript = useRpc("run.transcript", { run_id: run });
  return (
    <QueryView<RunTranscriptResult> query={transcript} isEmpty={(d) => d.length === 0} emptyTitle="This run has no transcript lines">
      {(events) => <Transcript run={run} line={line} events={events} />}
    </QueryView>
  );
}
