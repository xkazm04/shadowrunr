import { useMemo } from "react";

import { diagramFor, type Diagram } from "../diagram";
import type { Card } from "../sceneModel";
import { cardWindow } from "./onScreen";

function DiagramView({ d }: { d: Diagram }) {
  switch (d.kind) {
    case "flow":
      return (
        <div className="dia dia-flow">
          {d.steps.map((s, i) => (
            <div key={i} className="flow-step-wrap" style={{ animationDelay: `${0.25 + i * 0.45}s` }}>
              {i > 0 && <span className="flow-arrow">➝</span>}
              <span className="flow-step">{s}</span>
            </div>
          ))}
        </div>
      );
    case "figures": {
      const max = Math.max(1e-9, ...d.items.map((x) => Math.abs(x.value)));
      return (
        <div className="dia dia-figures">
          {d.items.map((x, i) => (
            <div key={i} className="fig-row">
              <span className="fig-label">{x.label}</span>
              <span className="fig-track">
                <span
                  className="fig-fill"
                  style={{ width: `${Math.max(1.5, (Math.abs(x.value) / max) * 100)}%`, animationDelay: `${0.2 + i * 0.25}s` }}
                />
              </span>
              <span className="fig-value">
                {x.value}
                {x.unit}
              </span>
            </div>
          ))}
        </div>
      );
    }
    case "gauge": {
      const c = 2 * Math.PI * 52;
      return (
        <div className="dia dia-gauge">
          <svg viewBox="0 0 140 140" width="180" height="180">
            <circle cx="70" cy="70" r="52" fill="none" stroke="#1f2a26" strokeWidth="14" />
            <circle
              className="gauge-arc"
              cx="70"
              cy="70"
              r="52"
              fill="none"
              stroke="#5fe0bf"
              strokeWidth="14"
              strokeLinecap="round"
              strokeDasharray={`${(d.value / 100) * c} ${c}`}
              transform="rotate(-90 70 70)"
              style={{ ["--c" as string]: c }}
            />
            <text x="70" y="80" textAnchor="middle" className="gauge-num">
              {d.value}%
            </text>
          </svg>
          <span className="gauge-label">{d.label}</span>
        </div>
      );
    }
    case "runes":
      return (
        <div className="dia dia-runes">
          <svg viewBox="-100 -60 200 120" className="rune-core">
            <circle r="26" fill="none" stroke="#5fe0bf" strokeWidth="1.5" strokeDasharray="3 5" className="rune-ring" />
            <polygon points="0,-16 14,8 -14,8" fill="none" stroke="#c9a45a" strokeWidth="2" />
            <circle r="4" fill="#5fe0bf" />
          </svg>
          {d.words.map((w, i) => {
            const a = (i / Math.max(1, d.words.length)) * Math.PI * 2 - Math.PI / 2;
            return (
              <span
                key={w}
                className="rune-word"
                style={{
                  left: `${50 + Math.cos(a) * 36}%`,
                  top: `${50 + Math.sin(a) * 34}%`,
                  animationDelay: `${0.2 + i * 0.3}s`,
                }}
              >
                {w}
              </span>
            );
          })}
        </div>
      );
  }
}

/** `show_card`: an explainer card with an animated diagram picked from its text. */
export function ExplainerCard({ card, now }: { card: Card | null; now: number }) {
  const d = useMemo(() => (card ? diagramFor(card.title, card.body) : null), [card]);
  const shown = cardWindow(card, now);
  if (!shown || !d) return null;
  const { value: c, leaving } = shown;
  return (
    <div className={`explainer${now - shown.since < 800 ? " entering" : ""}${leaving ? " leaving" : ""}`} key={c.id}>
      <div className="explainer-kicker">THE LICH EXPLAINS</div>
      <div className="explainer-title">{c.title}</div>
      <div className="explainer-body">{c.body}</div>
      <div className="explainer-diagram">
        <DiagramView d={d} />
      </div>
    </div>
  );
}
