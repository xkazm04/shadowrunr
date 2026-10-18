import { useEffect, useRef, useState } from "react";
import type { HealthReport } from "../contract";
import { useApi } from "../api/context";
import { Button, Dot, Empty, ErrorState, Inspector, KV, Loading, Toolbar, cx, useListNav } from "../ui/primitives";

/** Media-less mapping of the design's pre-flight board (.spark briefs WP5 §3). */
export const PF_LABELS: Record<string, string> = {
  "PF-01": "Engine fleet",
  "PF-02": "Character package",
  "PF-03": "Memory store",
  "PF-04": "Segment tables",
  "PF-05": "Platform rules",
  "PF-06": "Sim calibration",
  "PF-07": "Disk and runs dir",
  "PF-08": "Media stack",
};

const ORDER = { red: 0, amber: 1, green: 2 } as const;

export function PreflightView() {
  const { client } = useApi();
  const [reports, setReports] = useState<HealthReport[] | null>(null);
  const [ranAt, setRanAt] = useState<number | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [selected, setSelected] = useState<string | null>(null);

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      const r = await client.call("preflight.run", {});
      setReports([...r].sort((a, b) => a.check.localeCompare(b.check)));
      setRanAt(Date.now());
    } catch (e) {
      setError(e);
    } finally {
      setRunning(false);
    }
  };

  // Run once when the view opens; R re-runs (unless typing in a field).
  const runRef = useRef(run);
  runRef.current = run;
  const busy = useRef(false);
  busy.current = running;
  useEffect(() => {
    void runRef.current();
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (e.key.toLowerCase() !== "r" || e.ctrlKey || e.metaKey || e.altKey || busy.current) return;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) return;
      void runRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const rows = reports ?? [];
  const nav = useListNav(rows.length, { onMove: (i) => setSelected(rows[i]?.check ?? null), onActivate: (i) => setSelected(rows[i]?.check ?? null) });
  const sel = rows.find((r) => r.check === selected);
  const counts = { green: 0, amber: 0, red: 0 };
  for (const r of rows) counts[r.state]++;
  const worst = rows.length ? [...rows].sort((a, b) => ORDER[a.state] - ORDER[b.state])[0]!.state : null;

  return (
    <div className="view">
      <div className="main">
        <Toolbar
          title="Pre-flight"
          subtitle={
            reports ? (
              <>
                <Dot state={worst ?? "none"} /> {counts.green} green · {counts.amber} amber · {counts.red} red
                {ranAt && <span className="muted"> · {new Date(ranAt).toLocaleTimeString()}</span>}
              </>
            ) : (
              "PF-01..PF-08"
            )
          }
        >
          <Button variant="primary" onClick={() => void run()} disabled={running} kbd="R">
            {running ? "Running…" : "Run"}
          </Button>
        </Toolbar>
        <div className="content">
          {error ? (
            <ErrorState error={error} onRetry={() => void run()} />
          ) : running && !reports ? (
            <Loading label="Running checks" />
          ) : !reports ? (
            <Empty title="No pre-flight run yet">
              <Button variant="primary" onClick={() => void run()}>
                Run pre-flight
              </Button>
            </Empty>
          ) : (
            <div className="table" {...nav.containerProps} aria-label="Pre-flight checks">
              <div className="tr th">
                <span className="c-dot" />
                <span className="c-id">Check</span>
                <span className="c-name">Area</span>
                <span className="c-grow">Detail</span>
              </div>
              {rows.map((r, i) => (
                <div
                  key={r.check}
                  {...nav.rowProps(i)}
                  className={cx("tr", i === nav.active && "active", selected === r.check && "selected", `state-${r.state}`)}
                  onClick={() => setSelected(r.check)}
                >
                  <span className="c-dot">
                    <Dot state={r.state} />
                  </span>
                  <span className="c-id mono">{r.check}</span>
                  <span className="c-name">{PF_LABELS[r.check] ?? "—"}</span>
                  <span className="c-grow ellipsis muted">{r.detail}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
      {sel && (
        <Inspector title={`${sel.check} · ${PF_LABELS[sel.check] ?? ""}`} onClose={() => setSelected(null)}>
          <KV
            rows={[
              ["State", <span className="state-label"><Dot state={sel.state} /> {sel.state}</span>],
              ["Check", <span className="mono">{sel.check}</span>],
            ]}
          />
          <p className="prose">{sel.detail}</p>
          {sel.state !== "green" && (
            <p className="muted small">
              {sel.state === "red" ? "A red check blocks sim.start in the daemon." : "Amber does not block simulation runs."}
            </p>
          )}
        </Inspector>
      )}
    </div>
  );
}
