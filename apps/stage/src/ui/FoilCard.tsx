import { useMemo } from "react";

import { foilInfo } from "../lore";
import type { SceneState } from "../scene";
import { captionOpacity, wordTimings } from "../timing";
import { KineticText } from "./Caption";

function Emblem({ id, size }: { id: string; size: number }) {
  const f = foilInfo(id);
  const initials = f.name
    .split(" ")
    .filter((w) => w.toLowerCase() !== "the")
    .map((w) => w[0])
    .join("")
    .slice(0, 2);
  return (
    <svg className="foil-emblem" width={size} height={size} viewBox="0 0 64 64" style={{ color: f.color }}>
      <circle cx="32" cy="32" r="29" fill="#101413" stroke="currentColor" strokeWidth="3" />
      <circle cx="32" cy="32" r="23" fill="none" stroke="currentColor" strokeWidth="1" strokeDasharray="3 4" opacity="0.7" />
      <text x="32" y="40" textAnchor="middle" fontSize="22" fontWeight="700" fill="currentColor" fontFamily="Cinzel Variable, serif">
        {initials}
      </text>
    </svg>
  );
}

function useFoilLine(scene: SceneState, now: number) {
  const line = scene.foilLine;
  const timed = useMemo(() => (line ? wordTimings(line.text) : null), [line]);
  if (!line || !timed) return null;
  const elapsed = now - line.at;
  const opacity = captionOpacity(timed, elapsed);
  return opacity > 0 ? { line, elapsed, opacity } : null;
}

/** SEG-01: the summoned guest foil, with its latest line. */
export function FoilCard({ scene, now }: { scene: SceneState; now: number }) {
  const foil = scene.foil;
  const said = useFoilLine(scene, now);
  if (!foil) return <div className="panel foil-card empty" />;
  const f = foilInfo(foil.id);
  return (
    <div className={`panel foil-card${now - foil.since < 1200 ? " summoning" : ""}`} style={{ ["--foil" as string]: f.color }} key={foil.id}>
      <div className="panel-title">
        <span>Guest foil</span>
        <span className="panel-sub">original parody · not a real person</span>
      </div>
      <div className="foil-main">
        <Emblem id={foil.id} size={84} />
        <div>
          <div className="foil-name">{f.name}</div>
          <div className="foil-tagline">{f.tagline}</div>
        </div>
      </div>
      {said && (
        <div className="foil-bubble" key={said.line.id} style={{ opacity: said.opacity }}>
          <KineticText text={said.line.text} elapsed={said.elapsed} />
        </div>
      )}
    </div>
  );
}

/** SEG-02A: the foil shrinks to a "Skeptical Observer" badge (digest § 3). */
export function ObserverBadge({ scene, now }: { scene: SceneState; now: number }) {
  const foil = scene.foil;
  const said = useFoilLine(scene, now);
  if (!foil) return null;
  const f = foilInfo(foil.id);
  return (
    <div className="observer" style={{ ["--foil" as string]: f.color }}>
      <Emblem id={foil.id} size={52} />
      <div className="observer-body">
        <div className="observer-kicker">SKEPTICAL OBSERVER</div>
        <div className="observer-title">{f.name}</div>
        {said ? (
          <div className="observer-line" key={said.line.id} style={{ opacity: said.opacity }}>
            <KineticText text={said.line.text} elapsed={said.elapsed} />
          </div>
        ) : (
          <div className="observer-line dim">watching, unconvinced</div>
        )}
      </div>
    </div>
  );
}
