import type { EvidenceGrade } from "@shadowrunr/contract";
import type { MapModel } from "../hexgrid";
import { GRADE, isAlleged } from "../lore";
import type { SceneState } from "../sceneModel";
import { gradeTallies } from "../tallies";
import { KIND_LABEL, REVEAL_MS } from "./Hexmap";

const ORDER: EvidenceGrade[] = ["DOC", "REP", "CLM", "SPC"];

/** The side inspector for the most recently revealed hex. */
export function HexInspector({ map, scene, now }: { map: MapModel; scene: SceneState; now: number }) {
  const id = scene.revealOrder[scene.revealOrder.length - 1];
  const hex = id ? map.hexes.find((h) => h.id === id) : undefined;
  const st = id ? scene.hexes[id] : undefined;
  if (!hex || !st) {
    return (
      <div className="panel inspector">
        <div className="panel-title">
          <span>Hex inspector</span>
        </div>
        <div className="inspector-empty">The fog holds. Nothing has been audited yet.</div>
      </div>
    );
  }
  const g = GRADE[st.grade];
  const fresh = now - st.revealedAt < REVEAL_MS;
  const sources = hex.sources?.length ?? 0;
  return (
    <div className={`panel inspector${fresh ? " fresh" : ""}`} key={hex.id} style={{ ["--grade" as string]: g.color }}>
      <div className="panel-title">
        <span>Hex inspector · {hex.id}</span>
        <span className="panel-sub">{KIND_LABEL[hex.kind]}</span>
      </div>
      <div className="inspector-label">{hex.label}</div>
      <div className="inspector-grade">
        <span className="big-badge" style={{ borderColor: g.color, color: g.color }}>
          {g.code}
        </span>
        <div>
          <div className="inspector-grade-name" style={{ color: g.color }}>
            {g.name}
          </div>
          <div className="inspector-note">
            {isAlleged(st.grade) ? "Below DOC: stated as alleged, never as fact." : "A primary source is on file."}
            {sources > 0 ? ` ${sources} source${sources > 1 ? "s" : ""} on file.` : ""}
          </div>
        </div>
        {isAlleged(st.grade) && <span className="inspector-alleged">ALLEGED</span>}
      </div>
    </div>
  );
}

/** Community `!grade` tally per hex, against Malakor's grade. */
export function GradeTallyPanel({ scene }: { scene: SceneState }) {
  const rows = gradeTallies(scene)
    .filter((t) => t.total > 0)
    .sort((a, b) => b.total - a.total)
    .slice(0, 3);
  return (
    <div className="panel tally">
      <div className="panel-title">
        <span>Minion jury</span>
        <span className="panel-sub">!grade &lt;hex&gt; DOC|REP|CLM|SPC</span>
      </div>
      {rows.length === 0 && <div className="inspector-empty">No minion has graded a hex yet.</div>}
      {rows.map((t) => (
        <div key={t.hex} className={`tally-row${t.disputed ? " disputed" : ""}`}>
          <div className="tally-head">
            <span className="tally-hex">{t.hex}</span>
            {t.official && (
              <span className="tally-official" style={{ color: GRADE[t.official].color }}>
                Malakor: {t.official}
              </span>
            )}
            {t.majority && <span className="tally-majority">Jury: {t.majority}</span>}
            {t.disputed && <span className="disputed-tag">DISPUTED</span>}
            <span className="tally-n">{t.total}</span>
          </div>
          <div className="tally-bar">
            {ORDER.map((g) =>
              t.counts[g] > 0 ? (
                <span key={g} style={{ flexGrow: t.counts[g], background: GRADE[g].color }} title={`${g} ${t.counts[g]}`}>
                  {g} {t.counts[g]}
                </span>
              ) : null,
            )}
          </div>
        </div>
      ))}
    </div>
  );
}
