// Layer 1, Simulator: the winner's poster frame (l1.js `frame`) around A/1's scaled simulator
// on canvas (app.js:446-475). Data: tournaments.page and runs.page (each run row carries its
// verdict's health and red-line fails).
import { QueryView } from "../../kit";
import { int, pct } from "../../kit/events";
import { PosterFrame } from "../../kit/PosterFrame";
import { pageRoute, RouteLink } from "../../router";
import { useSimGrid, type SimGrid } from "./data";
import styles from "./Layer1.module.css";
import { MAX_TILES, summarise } from "./grid";
import { healthRamp } from "./ramp";
import { SimScreen } from "./SimScreen";

export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <Trials />;
}

function Doors() {
  const doors = [
    ["tournaments", "Tournaments"],
    ["runs", "Runs"],
    ["verdicts", "Verdicts"],
    ["heatmap", "Heatmap"],
  ] as const;
  return (
    <nav className={styles.doors} aria-label="Simulator pages">
      {doors.map(([seg, label]) => (
        <RouteLink key={seg} to={pageRoute("simulator", seg)} className={styles.door}>
          {label} <i aria-hidden="true">›</i>
        </RouteLink>
      ))}
    </nav>
  );
}

function Hud({ grid, total }: { grid: SimGrid; total: number }) {
  const s = summarise(grid.tiles);
  const shown = grid.tiles.length;
  return (
    <>
      <div className={styles.heading} data-role="simulator-heading">
        <div className={styles.kicker}>
          The tournament as a living grid · {int(shown)} tournaments × {int(s.runs)} runs
        </div>
        <div className={styles.hudtxt}>
          Each tile is one tournament, each cell one run, coloured by its share of rule checks held. Ember dot = a red-line fail.
          {total > shown && ` Latest ${MAX_TILES} of ${int(total)}.`}
        </div>
      </div>
      <aside className={styles.hud} data-role="simulator-hud" aria-label="Simulator summary">
        <div>
          <div className={styles.hudk}>Simulator</div>
          <div className={styles.big}>
            {int(s.runs)}
            <small>runs</small>
          </div>
          <div className={styles.hudtxt}>
            {int(shown)} tournaments · {int(s.red)} red-line fails · best {pct(s.best?.health)} · worst {pct(s.worst?.health)}
            {!grid.complete && " · loading"}
          </div>
        </div>
        <Doors />
        <div className={styles.legend} data-role="simulator-legend">
          <div className={styles.hudtxt}>Health legend</div>
          <div className={styles.ramp} style={{ background: healthRamp() }} />
          <div className={styles.tag}>
            <span>0% held</span>
            <span>100%</span>
          </div>
        </div>
      </aside>
    </>
  );
}

function Trials() {
  const grid = useSimGrid();
  return (
    <PosterFrame
      className={styles.root}
      role="simulator-l1"
      heading="Layer 1 · The Trials"
      credit="Tournament of tone, tier, segment and audience · "
      note="v0.2 · voiced and recorded"
      cta={{ to: pageRoute("simulator", "tournaments"), label: "Open the trials", role: "push-in" }}
    >
      <div className={styles.hero}>
        <SimScreen tiles={grid.tiles}>
          <div className={styles.overlay}>
            <QueryView
              query={grid.tournaments.query}
              isEmpty={(items) => items.length === 0}
              emptyTitle="No tournaments yet"
              emptyHint="Run one with `shadowrunr sim tournament`; the grid fills as runs finish."
            >
              {(items) => <Hud grid={grid} total={items.length} />}
            </QueryView>
          </div>
        </SimScreen>
      </div>
    </PosterFrame>
  );
}
