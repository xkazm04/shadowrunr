import { Sigil } from "../art/Sigil";
import { DECREE_FADE_MS, decreeAt } from "../decrees";
import { RANK_COLOR, RANK_TITLE } from "../lore";
import type { SceneState } from "../sceneModel";

export { DECREE_MS } from "../decrees";

/**
 * Highlighted and picked chat messages lift out of the lane into "decree" cards, one at a time
 * (see decrees.ts): the card whose turn it is, or, between picks, the last line heard, held
 * quietly. On air the picker's reason (production detail) is left out.
 */
export function Decrees({ scene, now, onAir = false }: { scene: SceneState; now: number; onAir?: boolean }) {
  const at = decreeAt(scene.decrees, now);
  const d = at?.turn.decree;
  const m = d ? scene.chats.get(d.msgId) : undefined;
  if (!at || !d || !m) return <div className="decrees" />;
  const { turn, held } = at;
  const age = now - turn.start;
  const leaving = !held && turn.end - now < DECREE_FADE_MS;
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
