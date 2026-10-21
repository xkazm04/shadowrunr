import { Sigil } from "../art/Sigil";
import { RANK_COLOR, RANK_TITLE } from "../lore";
import type { SceneState } from "../sceneModel";
import { decreeOn } from "./onScreen";

export { DECREE_MS } from "../decrees";

/**
 * Highlighted and picked chat messages lift out of the lane into "decree" cards, one at a time
 * (see decrees.ts): the card whose turn it is, or, between picks, the last line heard, held
 * quietly. On air the picker's reason (production detail) is left out.
 */
export function Decrees({ scene, now, onAir = false }: { scene: SceneState; now: number; onAir?: boolean }) {
  const shown = decreeOn(scene, now);
  if (!shown) return <div className="decrees" />;
  const { leaving } = shown;
  const { turn, msg: m, held } = shown.value;
  const d = turn.decree;
  const age = now - turn.start;
  const color = RANK_COLOR[m.viewer.rank];
  const label = held ? "LAST HEARD BY THE LICH" : d.kind === "highlight" ? "DECREE OF THE LICH" : "HEARD BY THE LICH";
  return (
    <div className="decrees">
      <div
        key={held ? `held-${d.key}` : d.key}
        className={`decree decree-${d.kind}${held ? " decree-held" : ""}${!held && age < 900 ? " lifting" : ""}${leaving ? " leaving" : ""}`}
        style={{ ["--rank" as string]: color }}
      >
        <div className="decree-head">
          <span className="decree-label">{label}</span>
          {!onAir && !held && d.reason && <span className="decree-reason">{d.reason}</span>}
        </div>
        <div className="decree-main">
          <Sigil rank={m.viewer.rank} size={64} />
          <div className="decree-text">
            <div className="decree-who">
              <span style={{ color }}>{m.viewer.name}</span>
              <span className="decree-rank">{RANK_TITLE[m.viewer.rank]}</span>
            </div>
            <div className="decree-quote">“{m.text}”</div>
          </div>
        </div>
      </div>
    </div>
  );
}
