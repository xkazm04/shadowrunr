import { useEffect, useMemo, useRef, useState } from "react";
import type { StreamEvent } from "../contract";
import { useApi, useRpc } from "../api/context";
import type { EventStreamStatus } from "../api/events";
import { clock, eventActor, eventText } from "../lib/format";
import { EVENT_FILTERS, annotationsByLine, eventGroup, summarizeRules, type Annotation, type EventFilter } from "../lib/transcript";
import { openExternal, stageReplayUrl } from "../lib/open";
import { Button, Chip, ErrorState, Inspector, Loading, Segmented, cx, useListNav } from "./primitives";

function AnnotationChip({ a }: { a: Annotation }) {
  if (a.kind === "rule") {
    const tone = a.state === "fail" ? "bad" : a.state === "pass" ? "ok" : "warn";
    return (
      <Chip tone={tone} title={`${a.rule}: ${a.state}`}>
        {a.state === "fail" ? "✗" : a.state === "pass" ? "✓" : "?"} {a.rule.replace(/^(red-line|role|situation):/, "")}
      </Chip>
    );
  }
  return (
    <Chip tone={a.value ? "ok" : "bad"} title={`${a.dimension}: ${a.value} · “${a.quote}”`}>
      {a.value ? "+" : "−"} {a.dimension}
    </Chip>
  );
}

/** Indicative layer: counts only. The named chips appear when the row is opened. */
function AnnotationSummary({ ann }: { ann: Annotation[] }) {
  const ruleFail = ann.filter((a) => a.kind === "rule" && a.state === "fail").length;
  const ruleOther = ann.filter((a) => a.kind === "rule" && a.state !== "fail").length;
  const qTrue = ann.filter((a) => a.kind === "quality" && a.value).length;
  const qFalse = ann.filter((a) => a.kind === "quality" && !a.value).length;
  const title = ann.map((a) => (a.kind === "rule" ? `${a.rule}: ${a.state}` : `${a.dimension}: ${a.value}`)).join("\n");
  return (
    <span className="chips" title={title}>
      {ruleFail > 0 && <Chip tone="bad">✗ {ruleFail}</Chip>}
      {ruleOther > 0 && <Chip tone="ok">✓ {ruleOther}</Chip>}
      {qFalse > 0 && <Chip tone="bad">− {qFalse}</Chip>}
      {qTrue > 0 && <Chip tone="ok">+ {qTrue}</Chip>}
    </span>
  );
}

/** Transcript + judge verdict for one run, as an inspector pane. */
export function TranscriptInspector(props: { runId: string; highlightLine?: number; onClose: () => void }) {
  const { runId } = props;
  const { config, events: eventSource } = useApi();
  const transcript = useRpc("run.transcript", { run_id: runId }, runId);
  const verdict = useRpc("run.verdict", { run_id: runId }, runId);
  const [filter, setFilter] = useState<EventFilter>("all");
  const [wide, setWide] = useState(false);
  const [live, setLive] = useState<{ events: StreamEvent[]; status: EventStreamStatus; detail?: string } | null>(null);
  const [expanded, setExpanded] = useState<number | null>(props.highlightLine ?? null);

  useEffect(() => {
    setLive(null);
    setExpanded(props.highlightLine ?? null);
  }, [runId, props.highlightLine]);

  const watching = live !== null;
  useEffect(() => {
    if (!watching) return;
    return eventSource.subscribe(
      runId,
      1,
      (ev) => setLive((l) => (l ? { ...l, events: [...l.events, ev] } : l)),
      (status, detail) => setLive((l) => (l ? { ...l, status, detail } : l)),
    );
  }, [watching, runId, eventSource]);

  const all = live ? live.events : (transcript.data ?? []);
  const notes = useMemo(() => annotationsByLine(verdict.data, runId), [verdict.data, runId]);
  const rows = useMemo(
    () => all.map((ev, i) => ({ ev, line: i + 1 })).filter((r) => EVENT_FILTERS[filter](r.ev)),
    [all, filter],
  );
  const highlightIdx = rows.findIndex((r) => r.line === props.highlightLine);
  const nav = useListNav(rows.length, {
    initial: highlightIdx,
    onActivate: (i) => {
      const line = rows[i]?.line ?? null;
      setExpanded((e) => (e === line ? null : line));
    },
  });
  const { setActive } = nav;
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (highlightIdx >= 0) setActive(highlightIdx);
  }, [highlightIdx, setActive, transcript.data]);

  // Live tail follows the newest event.
  useEffect(() => {
    if (live && listRef.current) listRef.current.scrollTop = listRef.current.scrollHeight;
  }, [live]);

  const sum = summarizeRules(verdict.data);
  const failed = verdict.data?.rules.filter((r) => r.state === "fail") ?? [];

  const jumpTo = (line: number) => {
    setFilter("all");
    const idx = all.findIndex((_, i) => i + 1 === line);
    if (idx >= 0) {
      setActive(idx);
      setExpanded(line);
    }
  };

  return (
    <Inspector
      wide={wide}
      title={
        <>
          <span className="mono">{runId}</span>
        </>
      }
      onClose={props.onClose}
      actions={
        <>
          <Button variant="ghost" onClick={() => setWide((w) => !w)} title="Widen the inspector">
            {wide ? "Narrow" : "Widen"}
          </Button>
          <Button variant="ghost" onClick={() => setLive(live ? null : { events: [], status: "connecting" })} title="Replay this run here through /events">
            {live ? "Stop" : "Watch"}
          </Button>
          <Button onClick={() => void openExternal(stageReplayUrl(config.stageUrl, runId))} title={`Open ${config.stageUrl}?run=${runId}`}>
            Replay on stage
          </Button>
        </>
      }
    >
      <div className="verdict-bar">
        {verdict.loading ? (
          <span className="muted">Loading verdict…</span>
        ) : verdict.data ? (
          <>
            <Chip tone={sum.fail ? "bad" : "ok"}>
              rules {sum.pass}/{sum.pass + sum.fail}
            </Chip>
            {sum.not_provoked > 0 && <Chip tone="neutral" title="not provoked: never a pass">NP {sum.not_provoked}</Chip>}
            {sum.not_evaluable > 0 && <Chip tone="warn" title="not evaluable: never a pass">NE {sum.not_evaluable}</Chip>}
            <Chip tone="neutral">judge {verdict.data.judge_tier}</Chip>
            {failed.map((r) => {
              const first = r.evidence?.[0];
              const line = first ? Number(first.slice(first.lastIndexOf("#") + 1)) : undefined;
              return (
                <Chip key={r.rule} tone="bad" onClick={line ? () => jumpTo(line) : undefined} title={r.evidence?.join(", ")}>
                  ✗ {r.rule}
                </Chip>
              );
            })}
            {verdict.data.notes && (
              <span className="verdict-note" title={verdict.data.notes}>
                {verdict.data.notes}
              </span>
            )}
          </>
        ) : (
          <span className="muted">No verdict for this run</span>
        )}
      </div>
      <div className="transcript-tools">
        <Segmented
          label="Event filter"
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: "All" },
            { value: "speech", label: "Speech" },
            { value: "chat", label: "Chat" },
            { value: "ops", label: "Ops" },
            { value: "flags", label: "Moderation" },
          ]}
        />
        {live && (
          <span className={cx("live-pill", live.status === "open" && "on")}>
            {live.status === "open" ? "● live" : live.status}
            {live.detail ? ` · ${live.detail}` : ""}
          </span>
        )}
        <span className="muted">{rows.length} events</span>
      </div>
      {transcript.error && !live ? (
        <ErrorState error={transcript.error} onRetry={transcript.reload} />
      ) : transcript.loading && !live ? (
        <Loading label="Loading transcript" />
      ) : (
        <div
          className="transcript"
          {...nav.containerProps}
          ref={(el) => {
            nav.containerProps.ref.current = el;
            listRef.current = el;
          }}
          aria-label="Transcript"
        >
          {rows.map((r, i) => {
            const ann = notes.get(r.line) ?? [];
            const open = expanded === r.line;
            const bad = ann.some((a) => (a.kind === "rule" && a.state === "fail") || (a.kind === "quality" && !a.value));
            return (
              <div
                key={r.line}
                {...nav.rowProps(i)}
                className={cx(
                  "t-row",
                  `ev-${eventGroup(r.ev)}`,
                  i === nav.active && "active",
                  r.line === props.highlightLine && "highlight",
                  open && "open",
                  bad && "flagged",
                )}
                onClick={() => setExpanded(open ? null : r.line)}
              >
                <span className="t-line mono">{r.line}</span>
                <span className="t-clock mono">{clock(r.ev.ts_ms)}</span>
                <span className="t-type">{eventActor(r.ev)}</span>
                <span className="t-text">{eventText(r.ev)}</span>
                <span className="t-notes">
                  {open ? ann.map((a, k) => <AnnotationChip key={k} a={a} />) : ann.length > 0 && <AnnotationSummary ann={ann} />}
                </span>
              </div>
            );
          })}
        </div>
      )}
    </Inspector>
  );
}
