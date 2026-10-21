// The evidence board (appendix (screens and board)): the case's cards on cork, face down until
// Malakor flips them, the strings he pins between them, the acts as regions, the thesis on top
// and the timeline strip along the bottom. Every string cites its record, and anything below DOC
// is drawn as alleged. Geometry is `board.ts` and `boardStrings.ts`; this is the drawing.

import { useMemo, type CSSProperties } from "react";

import type { Case, EvidenceGrade, Hex, HexRole } from "@shadowrunr/contract";
import { boardRegions, boardView, CARD_H, CARD_W, layoutBoard, regionOf, stripBox, THESIS_H, type BoardFrame, type BoardLayout } from "../board";
import { KIND_NAME, KIND_ROLE, REL_LABEL, ROLE_LABEL } from "../boardNames";
import { labelSpot, markLabelWidth, STRIP_INSET, stringGeom, stringLanes, timelineMarks, type Point, type StringGeom } from "../boardStrings";
import type { Box } from "../hexgrid";
import type { MapModel } from "../hexgrid";
import { GRADE, isAlleged } from "../lore";
import type { PinnedLink, SceneState } from "../sceneModel";

/** How long a card takes to flip face up, and a string to draw in (stream ms). */
export const FLIP_MS = 1400;
export const DRAW_MS = 1600;

/** A card's role glyph, drawn in a 24 px box centred on 0, 0. */
const ROLE_GLYPH: Record<HexRole, string> = {
  victim: "M4 -5 A4 4 0 1 1 -4 -5 A4 4 0 1 1 4 -5 Z M-8 9 Q-8 1 0 1 Q8 1 8 9 M9 -9 Q11 -6 9 -4 Q7 -6 9 -9 Z",
  promoter: "M-9 -3 L-3 -3 L7 -9 L7 9 L-3 3 L-9 3 Z M-6 3 L-4 9",
  shell: "M-10 8 L-10 -2 L-4 -8 L2 -2 L2 8 Z M2 8 L2 -6 L10 -6 L10 8",
  processor: "M-10 -6 L10 -6 L10 7 L-10 7 Z M-10 -2 L10 -2 M-6 3 L-1 3",
  regulator: "M-10 -4 L0 -10 L10 -4 Z M-7 -2 L-7 6 M-2 -2 L-2 6 M2 -2 L2 6 M7 -2 L7 6 M-10 8 L10 8",
  claim: "M-9 -8 L9 -8 L9 4 L-2 4 L-7 9 L-6 4 L-9 4 Z",
  wallet: "M-10 -6 L8 -6 L10 -3 L10 7 L-10 7 Z M4 0 L10 0 L10 4 L4 4 Z",
  exchange: "M-9 -4 L7 -4 M3 -8 L7 -4 L3 0 M9 4 L-7 4 M-3 0 L-7 4 L-3 8",
  person: "M4 -5 A4 4 0 1 1 -4 -5 A4 4 0 1 1 4 -5 Z M-8 9 Q-8 1 0 1 Q8 1 8 9",
  company: "M-8 9 L-8 -9 L4 -9 L4 9 M4 -3 L9 -3 L9 9 M-10 9 L10 9 M-5 -5 L-2 -5 M-5 0 L-2 0 M-5 4 L-2 4",
};

/** A card's label without the fictional marker (the ribbon says it). */
export function cardLabel(label: string): string {
  return label.replace(/\s*\(fictional\)\s*/gi, " ").trim();
}

/** The timeline strip's short form of a card's label. */
export function shortCardLabel(h: Hex, max = 18): string {
  const t = cardLabel(h.label).replace(/,.*$/, "");
  return t.length > max ? `${t.slice(0, max - 1).trimEnd()}…` : t;
}

/** The board's regions and card places for a case in the board screen's frame. */
export function boardLayoutFor(map: MapModel, caseFile: Case | null, frame: BoardFrame): BoardLayout {
  return layoutBoard(boardRegions(caseFile?.acts, map.hexes), frame);
}

const gradeStyle = (g: EvidenceGrade): CSSProperties => ({ ["--grade" as string]: GRADE[g].color });

/** What the board shows of a card: its grade once face up, and since when. */
function faceOf(scene: SceneState, h: Hex): { grade: EvidenceGrade; at: number | null } | null {
  const live = scene.hexes[h.id];
  if (live) return { grade: live.grade, at: live.revealedAt };
  return !h.fog && h.grade ? { grade: h.grade, at: null } : null;
}

function CardFace({ h, grade }: { h: Hex; grade: EvidenceGrade }) {
  const role = h.role ?? KIND_ROLE[h.kind];
  return (
    <div className="card-face">
      <div className="card-head">
        <svg className="card-glyph" viewBox="-12 -12 24 24" aria-hidden="true">
          <path d={ROLE_GLYPH[role]} />
        </svg>
        <span className="card-role">{h.role ? ROLE_LABEL[h.role] : KIND_NAME[h.kind]}</span>
        <span className="card-id">{h.id}</span>
      </div>
      <div className="card-label">{cardLabel(h.label)}</div>
      <div className="card-foot">
        <span className="card-date">{h.date ?? ""}</span>
        {isAlleged(grade) && <span className="card-alleged">ALLEGED</span>}
        <span className="card-grade">{grade}</span>
      </div>
    </div>
  );
}

/** A string label's two lines: what the string says, and the record it cites. */
function labelText(link: PinnedLink): { rel: string; cite: string; w: number } {
  const rel = REL_LABEL[link.rel].toUpperCase();
  const cite = isAlleged(link.grade) ? `${link.recordId} · alleged` : link.recordId;
  return { rel, cite, w: Math.max(rel.length * 8.6, cite.length * 6.6) + 22 };
}
const LABEL_H = 38;

/** Each drawn string's geometry and where its label rides, clear of the cards and of the labels
 * placed before it where the curve allows. */
function placeStrings(links: readonly PinnedLink[], cards: Record<string, Box>, s: number): ({ geom: StringGeom; at: Point } | null)[] {
  const lanes = stringLanes(links);
  const taken: Box[] = Object.values(cards);
  return links.map((l, i) => {
    const a = cards[l.from];
    const b = cards[l.to];
    const geom = a && b ? stringGeom(a, b, lanes[i]) : null;
    if (!geom) return null;
    const w = labelText(l).w * s;
    const h = LABEL_H * s;
    const at = labelSpot(geom, w, h, taken);
    taken.push({ x: at.x - w / 2, y: at.y - h / 2, w, h });
    return { geom, at };
  });
}

function StringLabel({ link, s }: { link: PinnedLink; s: number }) {
  const { rel, cite, w } = labelText(link);
  return (
    <g className="string-label" transform={`scale(${s.toFixed(3)})`}>
      <rect x={-w / 2} y={-LABEL_H / 2} width={w} height={LABEL_H} rx={5} />
      <text className="string-rel" y={-3} textAnchor="middle">
        {rel}
      </text>
      <text className="string-cite" y={12} textAnchor="middle">
        {cite}
      </text>
    </g>
  );
}

export function EvidenceBoard({
  map,
  scene,
  now,
  caseFile,
  layout,
  frame,
  focus = null,
  highlight = null,
  strip = true,
}: {
  map: MapModel;
  scene: SceneState;
  now: number;
  caseFile: Case | null;
  /** Where every card goes (`boardLayoutFor` in the board screen's frame), so each window that
   * draws the board places the cards alike. */
  layout: BoardLayout;
  /** This window: its size and what stands over it. */
  frame: BoardFrame;
  /** A card to zoom to with the cards strung to it (`show_screen` focus). */
  focus?: string | null;
  /** A card to mark as the open record's (the desk). */
  highlight?: string | null;
  /** Draw the timeline strip (the desk's window leaves it to the browser in front). */
  strip?: boolean;
}) {
  const frameKey = JSON.stringify(frame);
  const links = scene.links;
  // frameKey stands for `frame` (a new object each render with the same numbers)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const view = useMemo(() => boardView(layout, frame, focus, links), [layout, frameKey, focus, links]);
  const hexById = useMemo(() => new Map(map.hexes.map((h) => [h.id, h])), [map]);
  const faces = new Map(map.hexes.map((h) => [h.id, faceOf(scene, h)]));
  const faceUp: Record<string, EvidenceGrade> = {};
  for (const [id, f] of faces) if (f) faceUp[id] = f.grade;
  const latest = scene.revealOrder[scene.revealOrder.length - 1] ?? null;
  const lastLink = links[links.length - 1];
  const litAct = regionOf(layout, focus ?? highlight ?? lastLink?.to ?? latest ?? "")?.name ?? null;
  const s = layout.scale;
  const labelScale = Math.max(0.75, s);
  const strings = useMemo(() => placeStrings(links, layout.cards, labelScale), [links, layout, labelScale]);
  const box = stripBox(frame);
  const records = Object.values(scene.records);
  const timeline = timelineMarks(map.hexes, faceUp, records, box.w, shortCardLabel);
  const dim = (id: string) => view.ring.length > 0 && !view.ring.includes(id);
  const under = (id: string) => view.under.includes(id);
  const thesis = caseFile?.thesis ?? caseFile?.title ?? "The case";
  // The band ends left of anything that stands over it (the observer badge).
  const bandEnd = Math.min(frame.w, ...(frame.avoid ?? []).filter((a) => a.y < THESIS_H && a.y + a.h > 0 && a.x > frame.w / 2).map((a) => a.x - 12));

  return (
    <div className={`panel board${view.ring.length > 0 ? " zoomed" : ""}`} data-cards={Object.keys(layout.cards).length}>
      <div className="board-zoom" style={{ width: layout.w, height: layout.h, transform: `translate(${view.fit.x.toFixed(1)}px, ${view.fit.y.toFixed(1)}px) scale(${view.fit.scale.toFixed(4)})` }}>
        {layout.regions.map((r, i) => (
          <div
            key={r.name}
            className={`board-act${r.name === litAct ? " lit" : ""}`}
            style={{ left: r.box.x, top: r.box.y, width: r.box.w, height: r.box.h, ["--act-s" as string]: s }}
          >
            <span className="board-act-name">
              <b>{["I", "II", "III", "IV", "V", "VI", "VII", "VIII"][i] ?? i + 1}</b> {r.name}
            </span>
          </div>
        ))}

        {Object.entries(layout.cards).map(([id, b]) => {
          const h = hexById.get(id);
          if (!h) return null;
          const f = faces.get(id) ?? null;
          const flipping = f?.at != null && now - f.at < FLIP_MS;
          const hint = scene.hints[id];
          const hinted = !f && hint !== undefined && now - hint.lastAt < 60000;
          const cls = [
            "board-card",
            f ? "face-up" : "face-down",
            flipping ? "flipping" : "",
            f && isAlleged(f.grade) ? "alleged" : "",
            id === latest ? "latest" : "",
            id === focus ? "in-focus" : "",
            id === highlight ? "on-record" : "",
            hinted ? "hinted" : "",
            dim(id) ? "dimmed" : "",
            under(id) ? "under" : "",
          ].filter(Boolean);
          return (
            <div
              key={id}
              className={cls.join(" ")}
              data-hex={id}
              style={{ left: b.x, top: b.y, width: CARD_W, height: CARD_H, transform: `scale(${(b.w / CARD_W).toFixed(4)})`, ...(f ? gradeStyle(f.grade) : {}) }}
            >
              <i className="card-pin" aria-hidden="true" />
              {(!f || flipping) && (
                <div className="card-back">
                  <span className="card-id">{id}</span>
                  <span className="card-fog">?</span>
                </div>
              )}
              {f && <CardFace h={h} grade={f.grade} />}
              {hinted && <span className="card-hint">{hint.count}</span>}
            </div>
          );
        })}

        <svg className="board-strings" width={layout.w} height={layout.h} viewBox={`0 0 ${layout.w} ${layout.h}`} aria-hidden="true">
          {links.map((l, i) => {
            const placed = strings[i];
            if (!placed || under(l.from) || under(l.to)) return null;
            const g = placed.geom;
            const drawing = now - l.at < DRAW_MS;
            const faded = view.ring.length > 0 && !(view.ring.includes(l.from) || view.ring.includes(l.to));
            const cls = ["board-string", `rel-${l.rel}`, isAlleged(l.grade) ? "alleged" : "", drawing ? "drawing" : "", faded ? "dimmed" : ""];
            const mask = `string-draw-${i}`;
            return (
              <g key={l.key} className={cls.filter(Boolean).join(" ")} style={gradeStyle(l.grade)} data-link={l.key}>
                {drawing && (
                  <mask id={mask} maskUnits="userSpaceOnUse" x={0} y={0} width={layout.w} height={layout.h}>
                    <path d={g.d} className="string-reveal" pathLength={1} />
                  </mask>
                )}
                <g mask={drawing ? `url(#${mask})` : undefined}>
                  <path d={g.d} className="string-line" />
                </g>
                <polygon points={g.head} className="string-head" />
                <g transform={`translate(${placed.at.x.toFixed(1)} ${placed.at.y.toFixed(1)})`}>
                  <StringLabel link={l} s={labelScale} />
                </g>
              </g>
            );
          })}
        </svg>
      </div>

      <div className="board-thesis" style={{ right: frame.w - bandEnd }}>
        {caseFile?.fictional && <span className="board-fiction">FICTIONAL CASE</span>}
        <span className="board-question">
          <span className="board-kicker">
            The question
            <span className="board-count">
              {Object.keys(faceUp).length}/{map.hexes.length} face up · {links.length} {links.length === 1 ? "string" : "strings"}
            </span>
          </span>
          <span className="board-thesis-text">{thesis}</span>
        </span>
      </div>

      {strip && box.w > 120 && timeline.from !== null && (
        <div className="board-strip" style={{ left: box.x, top: box.y, width: box.w, height: box.h }}>
          <i className="strip-axis" aria-hidden="true" />
          {timeline.from && <span className="strip-end start">{timeline.from}</span>}
          {timeline.to && <span className="strip-end end">{timeline.to}</span>}
          {timeline.marks.map((m) => {
            const half = markLabelWidth(m.label) / 2;
            const lx = Math.min(Math.max(m.x, STRIP_INSET - 8 + half), box.w - STRIP_INSET + 8 - half);
            return (
              <div
                key={`${m.kind}-${m.id}`}
                className={`strip-mark ${m.kind}${m.id === focus || m.id === latest ? " hot" : ""}`}
                style={{ left: m.x, ...gradeStyle(m.grade) }}
              >
                <i aria-hidden="true" />
                {m.row !== null && (
                  <span className={`strip-label row-${m.row}`} style={{ left: lx - m.x }}>
                    {m.label}
                  </span>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
