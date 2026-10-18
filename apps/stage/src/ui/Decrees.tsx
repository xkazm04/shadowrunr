import { Sigil } from "../art/Sigil";
import { RANK_COLOR, RANK_TITLE } from "../lore";
import type { SceneState } from "../scene";

export const DECREE_MS = { highlight: 9000, picked: 6500 } as const;

/** Highlighted and picked chat messages lift out of the lane into "decree" cards. */
export function Decrees({ scene, now }: { scene: SceneState; now: number }) {
  const live = scene.decrees.filter((d) => now >= d.at && now - d.at < DECREE_MS[d.kind]);
  return (
    <div className="decrees">
      {live.map((d, i) => {
        const m = scene.chats[d.msgId];
        if (!m) return null;
        const age = now - d.at;
        const leaving = DECREE_MS[d.kind] - age < 500;
        const depth = live.length - 1 - i;
        const color = RANK_COLOR[m.viewer.rank];
        return (
          <div
            key={d.key}
            className={`decree decree-${d.kind}${age < 900 ? " lifting" : ""}${leaving ? " leaving" : ""}`}
            style={{
              ["--rank" as string]: color,
              transform: `translateY(${-depth * 26}px) scale(${1 - depth * 0.05})`,
              zIndex: 10 - depth,
              opacity: depth > 0 ? 0.75 - depth * 0.15 : undefined,
            }}
          >
            <div className="decree-head">
              <span className="decree-label">{d.kind === "highlight" ? "DECREE OF THE LICH" : "HEARD BY THE LICH"}</span>
              {d.reason && <span className="decree-reason">{d.reason}</span>}
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
        );
      })}
    </div>
  );
}
