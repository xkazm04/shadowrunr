import type { Job, SceneState } from "../sceneModel";

export function jobPhase(status: string): "queued" | "running" | "done" | "failed" {
  const s = status.toLowerCase();
  if (/(fail|error|abort|cancel|timeout|lost)/.test(s)) return "failed";
  if (/(done|complete|finish|success|ok|returned)/.test(s)) return "done";
  if (/(run|work|progress|active|start)/.test(s)) return "running";
  return "queued";
}

const ICON = { queued: "◇", running: "◈", done: "✓", failed: "✕" } as const;

function text(j: Job): string {
  const phase = jobPhase(j.status);
  if ((phase === "done" || phase === "failed") && j.summary) return j.summary;
  return j.brief ?? j.summary ?? j.status;
}

/** The minion-dispatch ticker: background research and memory jobs, newest first. */
export function JobTicker({ scene, now }: { scene: SceneState; now: number }) {
  const jobs = [...scene.jobs].sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 5);
  return (
    <div className="ticker">
      <div className="ticker-label">
        <span className="ticker-dot" />
        MINION DISPATCH
      </div>
      <div className="ticker-items">
        {jobs.length === 0 && <span className="ticker-idle">No minions abroad. The Spire is quiet.</span>}
        {jobs.map((j) => {
          const phase = jobPhase(j.status);
          return (
            <div key={j.id} className={`job job-${phase}${now - j.updatedAt < 1200 ? " pulse" : ""}`}>
              <span className="job-icon">{ICON[phase]}</span>
              <span className="job-id">{j.id}</span>
              <span className="job-text">{text(j)}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
