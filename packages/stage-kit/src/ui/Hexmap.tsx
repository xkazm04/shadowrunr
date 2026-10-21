import { useMemo } from "react";

import type { Case, EvidenceGrade, Hex, HexKind } from "@shadowrunr/contract";
import { axialToPixel, fitMap, hexPoints, mapBounds, zoomMap, type Box, type HexFrame, type MapFit, type MapModel } from "../hexgrid";
import { GRADE, isAlleged } from "../lore";
import type { SceneState } from "../sceneModel";

export const REVEAL_MS = 1700;
/** The hexes' circumradius in map units (a fitted map scales it). */
export const HEX_SIZE = 74;
const SIZE = HEX_SIZE;
/** The legend band under the hexes, and the padding around them, in panel pixels. */
export const HEX_LEGEND_H = 46;
export const HEX_PAD = { x: 20, y: 16 } as const;

/** The box a framed panel fits its hexes into: inside the padding, above the legend. */
export function hexArea(frame: HexFrame): Box {
  return { x: HEX_PAD.x, y: HEX_PAD.y, w: frame.w - 2 * HEX_PAD.x, h: frame.h - HEX_PAD.y - HEX_LEGEND_H };
}

/** Where the legend starts: right of anything that stands over the legend band. */
function legendLeft(frame: HexFrame): number {
  const band = frame.h - HEX_LEGEND_H;
  let left: number = HEX_PAD.x;
  for (const a of frame.avoid ?? []) if (a.y + a.h > band && a.x < frame.w / 2) left = Math.max(left, a.x + a.w + 16);
  return left;
}

const KIND_GLYPH: Record<HexKind, string> = {
  entity: "M-9 6 L-9 -4 L0 -10 L9 -4 L9 6 Z M-5 6 L-5 0 M0 6 L0 0 M5 6 L5 0",
  wallet: "M-10 -6 L8 -6 L10 -3 L10 7 L-10 7 Z M4 0 L10 0 L10 4 L4 4 Z",
  domain: "M0 -10 A10 10 0 1 0 0.01 -10 Z M-10 0 L10 0 M0 -10 Q-6 0 0 10 Q6 0 0 -10",
  shell_company: "M-10 8 L-10 -2 L-4 -8 L2 -2 L2 8 Z M2 8 L2 -6 L10 -6 L10 8",
  claim: "M-9 -8 L9 -8 L9 4 L-2 4 L-7 9 L-6 4 L-9 4 Z",
};

const LEGEND_NAME: Record<EvidenceGrade, string> = {
  DOC: "primary source",
  REP: "press report",
  CLM: "community claim",
  SPC: "speculation",
};

export const KIND_LABEL: Record<HexKind, string> = {
  entity: "Entity",
  wallet: "Wallet",
  domain: "Domain",
  shell_company: "Shell company",
  claim: "Claim",
};

/** Split a label into at most two short lines for the inside of a hex. */
export function hexLabelLines(label: string, max = 17): string[] {
  const words = label.replace(/\s*\(fictional\)\s*/i, " ").trim().split(/\s+/);
  const lines: string[] = [];
  let cur = "";
  for (const w of words) {
    if ((cur + " " + w).trim().length <= max) cur = (cur + " " + w).trim();
    else {
      if (cur) lines.push(cur);
      cur = w.length > max ? w.slice(0, max - 1) + "…" : w;
    }
    if (lines.length === 2) break;
  }
  if (lines.length < 2 && cur) lines.push(cur);
  if (lines.length === 2 && words.join(" ").length > lines.join(" ").length + 1) {
    lines[1] = lines[1]!.slice(0, max - 1).replace(/\s+\S*$/, "") + "…";
  }
  return lines.slice(0, 2);
}

function GradeBadge({ grade, x, y }: { grade: EvidenceGrade; x: number; y: number }) {
  const g = GRADE[grade];
  return (
    <g className="grade-badge" transform={`translate(${x} ${y})`}>
      <rect x="-22" y="-11" width="44" height="22" rx="4" fill="#07100d" stroke={g.color} strokeWidth="2" />
      <text y="5" textAnchor="middle" fill={g.color} fontSize="14" fontWeight="700" fontFamily="IBM Plex Mono, monospace">
        {grade}
      </text>
    </g>
  );
}

export function Hexmap({
  map,
  scene,
  now,
  caseFile,
  frame,
  highlight = null,
  focus = null,
}: {
  map: MapModel;
  scene: SceneState;
  now: number;
  caseFile: Case | null;
  /** Fit the hexes into this panel around what stands over it (absent: centred in the CSS box). */
  frame?: HexFrame;
  /** A hex to mark as the open record's (the desk screen). */
  highlight?: string | null;
  /** A hex to zoom to with its ring of neighbours (`show_screen` focus; needs `frame`). The rest
   * of the map dims, and the move from the overview and back is a CSS transition. */
  focus?: string | null;
}) {
  const frameKey = frame ? JSON.stringify(frame) : "";
  const { pos, viewBox, fit } = useMemo(() => {
    const pos = new Map<string, { x: number; y: number }>();
    for (const h of map.hexes) pos.set(h.id, axialToPixel(h.q, h.r, SIZE));
    if (frame) {
      const f = fitMap(map.hexes, SIZE, hexArea(frame), frame.avoid, frame.prefer);
      return { pos, viewBox: `0 0 ${frame.w} ${frame.h}`, fit: f };
    }
    const b = mapBounds(map.hexes, SIZE);
    const pad = 30;
    return { pos, viewBox: `${b.minX - pad} ${b.minY - pad} ${b.maxX - b.minX + 2 * pad} ${b.maxY - b.minY + 2 * pad}`, fit: null };
    // frameKey stands for `frame` (a new object each render with the same numbers)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map, frameKey]);
  const zoom = useMemo(
    () => (frame && focus ? zoomMap(map.hexes, SIZE, focus, hexArea(frame), frame.avoid, frame.prefer) : null),
    // frameKey stands for `frame`, as above
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [map, frameKey, focus],
  );
  const view: MapFit | null = zoom?.fit ?? fit;
  const points = useMemo(() => hexPoints(SIZE - 3), []);
  const inner = useMemo(() => hexPoints(SIZE - 12), []);
  const latest = scene.revealOrder[scene.revealOrder.length - 1] ?? null;
  const zoomClass = (id: string) => {
    if (!zoom) return "";
    if (id === focus) return " in-focus";
    if (zoom.ring.includes(id)) return "";
    return zoom.under.includes(id) ? " dimmed under" : " dimmed";
  };
  const revealedCount = map.hexes.filter((h) => scene.hexes[h.id] || h.grade).length;

  const stateOf = (h: Hex) => {
    const live = scene.hexes[h.id];
    if (live) return { grade: live.grade, revealedAt: live.revealedAt as number | null };
    // A case may ship hexes that are already revealed (fog: false with a grade).
    if (!h.fog && h.grade) return { grade: h.grade, revealedAt: null };
    return null;
  };

  return (
    <div className="panel hexmap">
      {caseFile?.fictional && <div className="fiction-ribbon">FICTIONAL CASE</div>}
      <svg className={`hex-svg${fit ? " fitted" : ""}${zoom ? " zoomed" : ""}`} viewBox={viewBox} preserveAspectRatio={fit ? "xMinYMin meet" : "xMidYMid meet"}>
        <defs>
          <radialGradient id="fog-grad" cx="50%" cy="40%" r="70%">
            <stop offset="0%" stopColor="#2b3431" />
            <stop offset="100%" stopColor="#0c100f" />
          </radialGradient>
          <pattern id="fog-noise" width="22" height="22" patternUnits="userSpaceOnUse">
            <circle cx="4" cy="5" r="1.2" fill="#9fb3ab" opacity="0.18" />
            <circle cx="15" cy="12" r="1.6" fill="#9fb3ab" opacity="0.12" />
            <circle cx="9" cy="18" r="0.9" fill="#9fb3ab" opacity="0.2" />
          </pattern>
          <filter id="hex-glow" x="-30%" y="-30%" width="160%" height="160%">
            <feGaussianBlur stdDeviation="6" result="b" />
            <feMerge>
              <feMergeNode in="b" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {/* A CSS transform (not the attribute), so the zoom in and out is a transition. */}
        <g className="hex-zoom" style={view ? { transform: `translate(${view.x.toFixed(1)}px, ${view.y.toFixed(1)}px) scale(${view.scale.toFixed(4)})` } : undefined}>
          <g className="hex-links">
            {map.links.map(([a, b]) => {
              const pa = pos.get(a);
              const pb = pos.get(b);
              if (!pa || !pb) return null;
              const lit = !!(stateOf(map.hexes.find((h) => h.id === a)!) && stateOf(map.hexes.find((h) => h.id === b)!));
              return <line key={`${a}-${b}`} x1={pa.x} y1={pa.y} x2={pb.x} y2={pb.y} className={lit ? "link lit" : "link"} />;
            })}
        </g>

        {map.hexes.map((h) => {
          const p = pos.get(h.id)!;
          const st = stateOf(h);
          const hint = scene.hints[h.id];
          const revealing = st?.revealedAt != null && now - st.revealedAt < REVEAL_MS;
          const hinted = !st && hint !== undefined && now - hint.lastAt < 60000;
          const color = st ? GRADE[st.grade].color : "#3b4642";
          const lines = hexLabelLines(h.label);
          return (
            <g
              key={h.id}
              transform={`translate(${p.x} ${p.y})`}
              className={`hex${st ? " revealed" : " fogged"}${revealing ? " revealing" : ""}${h.id === latest ? " focus" : ""}${hinted ? " hinted" : ""}${st && isAlleged(st.grade) ? " alleged" : ""}${h.id === highlight ? " on-record" : ""}${zoomClass(h.id)}`}
              style={{ ["--grade" as string]: color }}
            >
              <g className="hex-body">
                <polygon points={points} className="hex-shape" fill={st ? `${color}22` : "url(#fog-grad)"} stroke={color} />
                {!st && <polygon points={points} fill="url(#fog-noise)" className="fog-noise" />}
                {st && <polygon points={inner} fill="none" stroke={color} strokeOpacity="0.35" strokeWidth="1" />}
                <text className="hex-id" y={st ? -40 : -8} textAnchor="middle">
                  {h.id}
                </text>
                {st ? (
                  <>
                    <path d={KIND_GLYPH[h.kind]} transform="translate(0 -18) scale(1.1)" className="kind-glyph" stroke={color} />
                    {lines.map((l, i) => (
                      <text key={i} className="hex-label" y={6 + i * 16} textAnchor="middle">
                        {l}
                      </text>
                    ))}
                    <GradeBadge grade={st.grade} x={0} y={48} />
                  </>
                ) : (
                  <text className="fog-mark" y={20} textAnchor="middle">
                    ?
                  </text>
                )}
              </g>
              {st && isAlleged(st.grade) && (
                <g className="alleged-stamp" transform="translate(0 -19) rotate(-10)">
                  <rect x="-40" y="-11" width="80" height="22" rx="3" />
                  <text y="5" textAnchor="middle">
                    ALLEGED
                  </text>
                </g>
              )}
              {hinted && (
                <g className="hint-badge" transform={`translate(${SIZE * 0.55} ${-SIZE * 0.62})`}>
                  <circle r="13" />
                  <text y="5" textAnchor="middle">
                    {hint.count}
                  </text>
                </g>
              )}
              {revealing && <polygon points={points} className="reveal-flash" />}
            </g>
          );
        })}
        </g>
      </svg>
      <div className="hex-legend" style={frame ? { left: legendLeft(frame) } : undefined}>
        {(Object.keys(GRADE) as EvidenceGrade[]).map((g) => (
          <span key={g} className="legend-item">
            <span className="legend-swatch" style={{ borderColor: GRADE[g].color, color: GRADE[g].color }}>
              {g}
            </span>
            {LEGEND_NAME[g]}
          </span>
        ))}
        <span className="legend-count">
          {revealedCount}/{map.hexes.length} burned through the fog
        </span>
      </div>
    </div>
  );
}
