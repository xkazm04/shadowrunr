// Brain layer 1: the winner's poster frame around A/1's oscilloscope wall (app.js:333-391).
// Lanes per run cell, the memory curve, the first-word gauge, role lamps and the picker funnel,
// each a lit panel that pushes in to its nested page. Canvases draw on input change only.
import { useCallback } from "react";
import type { TournamentsPage } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { RouteLink, pageRoute } from "../../router";
import { DegradedState, EmptyState } from "../../ui";
import { useBrainWindow, type BrainWindow } from "./useBrain";
import { FunnelChart, Gauge, Lamps } from "./Instruments";
import { Plot } from "./Plot";
import type { BrainModel } from "./brainModel";
import { drawMemory, drawScope, type Ctx, type Size } from "./draw";
import { int } from "./events";
import { BRAIN, paletteVars, roleColor } from "./palette";
import styles from "./Layer1.module.css";

const ENGINE = pageRoute("brain", "engine");
const TRANSCRIPTS = pageRoute("brain", "transcripts");

function Brand() {
  return (
    <div className={styles.brand}>
      <svg viewBox="0 0 40 40" aria-hidden="true">
        <path d="M6 16L9 5l7 7 4-9 4 9 7-7 3 11z" fill="currentColor" />
        <ellipse cx="20" cy="26" rx="11" ry="10" fill="none" stroke="currentColor" strokeWidth="3" />
        <circle cx="16" cy="25" r="2.4" fill="currentColor" />
        <circle cx="24" cy="25" r="2.4" fill="currentColor" />
      </svg>
      <b>Shadowrunr OS</b>
      <span>Season 0 · v0.1</span>
    </div>
  );
}

/** A/1's scene backdrop: near-black with an ember glow up and left. */
function Backdrop() {
  return (
    <svg className={styles.backdrop} viewBox="0 0 1600 810" preserveAspectRatio="none" aria-hidden="true" data-role="brain-backdrop">
      <defs>
        <radialGradient id="brG" cx="35%" cy="35%" r="70%">
          <stop offset="0" stopColor={BRAIN.glowCore} />
          <stop offset="1" stopColor={BRAIN.glowEdge} />
        </radialGradient>
      </defs>
      <rect width="1600" height="810" fill={BRAIN.backdrop} />
      <rect width="1600" height="810" fill="url(#brG)" />
    </svg>
  );
}

function Scene({ model }: { model: BrainModel }) {
  const scope = useCallback((c: Ctx, s: Size) => drawScope(c, s, model.lanes), [model.lanes]);
  const memory = useCallback((c: Ctx, s: Size) => drawMemory(c, s, model.memory, model.memMax), [model.memory, model.memMax]);
  const { latency, funnel } = model;
  const sec = (ms: number | undefined) => (ms === undefined ? "–" : (ms / 1000).toFixed(1));
  return (
    <>
      <RouteLink to={TRANSCRIPTS} className={`${styles.panel} ${styles.scope}`} data-role="brain-scope" aria-label={`Core thread pulses: ${model.calls} engine calls across ${model.runs} runs. Open the transcripts`}>
        <Plot draw={scope} className={styles.fill} role="brain-scope-canvas" label="Ledger pulses, one lane per core tier and audience" />
        <span className={styles.sweepTrack} aria-hidden="true">
          <i className={styles.sweep} />
        </span>
        <span className={styles.hud}>
          <span className={`${styles.hudk} ${styles.ember}`}>Core thread · live ledger pulses</span>
          <span className={`${styles.big} ${styles.bigScope}`}>
            {int(model.calls)}
            <small>calls · {int(model.runs)} runs</small>
          </span>
        </span>
        <span className={styles.legend} data-role="brain-legend">
          {model.roles.map((r) => (
            <span key={r.role} className={styles.tag}>
              <i style={{ background: roleColor(r.role) }} />
              {r.role.replace("_", " ")}
            </span>
          ))}
        </span>
      </RouteLink>
      <RouteLink to={ENGINE} className={`${styles.panel} ${styles.gaugePanel}`} data-role="brain-latency" aria-label="Latency: core first words p50 and p95. Open the engine">
        <Gauge latency={latency} />
        <span className={styles.gaugeText}>
          <span className={`${styles.big} ${styles.bigGauge}`}>
            {sec(latency.p50)}
            <small>s p50</small>
          </span>
          <span className={styles.hudtxt}>
            first words · p95 {sec(latency.p95)} s · n={latency.n}
          </span>
        </span>
      </RouteLink>
      <RouteLink to={TRANSCRIPTS} className={`${styles.panel} ${styles.memPanel}`} data-role="brain-memory" aria-label="Memory growth: moments kept, cumulative per run">
        <Plot draw={memory} className={styles.fill} role="brain-memory-canvas" label="Memory moments kept over stream time" />
        <span className={styles.hud}>
          <span className={styles.hudk}>Memory · moments kept</span>
          <span className={`${styles.big} ${styles.bigMem}`}>{int(model.memoryTotal)}</span>
        </span>
      </RouteLink>
      <RouteLink to={ENGINE} className={`${styles.panel} ${styles.lampsPanel}`} data-role="brain-pool" aria-label="Worker pool: calls and errors per role. Open the engine">
        <span className={`${styles.hud} ${styles.hudk}`}>Worker pool · calls by role</span>
        <Lamps roles={model.roles} />
      </RouteLink>
      <RouteLink to={TRANSCRIPTS} className={`${styles.panel} ${styles.funnelPanel}`} data-role="brain-picker" aria-label="The picker funnel: chat in, moderation, picked, answered">
        <span className={`${styles.hud} ${styles.hudk}`}>Picker funnel · {funnel.runs} runs</span>
        <FunnelChart funnel={funnel} />
      </RouteLink>
      <div className={styles.doors}>
        <RouteLink to={ENGINE} className={styles.door} data-role="brain-door-engine">
          Engine <i>›</i>
        </RouteLink>
        <RouteLink to={TRANSCRIPTS} className={styles.door} data-role="brain-door-transcripts">
          Transcripts <i>›</i>
        </RouteLink>
      </div>
    </>
  );
}

function Hero({ win }: { win: BrainWindow }) {
  return (
    <QueryView<TournamentsPage>
      query={win.tournaments}
      isEmpty={(d) => d.items.length === 0}
      emptyTitle="No simulation runs yet"
      emptyHint="Run a tournament and the brain's pulses, memory and picker funnel draw here."
    >
      {() =>
        win.model ? (
          <>
            {win.model.runs === 0 && <EmptyState title="The newest tournament has no runs yet" />}
            {win.degraded && <DegradedState title="Part of the brain did not load" hint={win.degraded.errorView?.title} onRetry={win.degraded.reload} />}
            <Scene model={win.model} />
          </>
        ) : win.degraded ? (
          <DegradedState title="The brain's runs did not load" hint={win.degraded.errorView?.title} onRetry={win.degraded.reload} />
        ) : null
      }
    </QueryView>
  );
}

/** Hidden under a nested page: nothing renders and nothing is watched. */
export function Layer1({ active }: { readonly active: boolean }) {
  return active ? <Wall /> : null;
}

function Wall() {
  const win = useBrainWindow();
  return (
    <div className={styles.root} style={paletteVars()} data-role="brain-layer1">
      <h2 className={styles.srOnly}>Layer 1 · The Brain</h2>
      <div className={styles.top}>
        <Brand />
        {win.truncated && <span className={styles.flag}>Newest tournament · first pages only</span>}
      </div>
      <div className={styles.hero} data-role="brain-hero" data-tournament={win.tournamentId}>
        <Backdrop />
        <Hero win={win} />
      </div>
      <div className={styles.foot}>
        <p className={styles.credit}>
          Directed by the core thread · {win.tournamentId ?? "no tournament yet"}
          <span>v0.1 · media-less phase: voice, avatar and broadcast come next · stylised illustration</span>
        </p>
        <RouteLink to={ENGINE} className={styles.cta} data-role="push-in">
          <span>Open the engine and transcripts</span>
          <i />
        </RouteLink>
      </div>
    </div>
  );
}
